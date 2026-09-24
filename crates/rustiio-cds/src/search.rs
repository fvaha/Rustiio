//! DLNA `Search` akcija: UPnP kriterij → FTS upit u bazi → DIDL.
//!
//! TV-i (i Serviio klijenti) šalju kriterij tipa
//! `upnp:class derivedfrom "object.item.videoItem" and dc:title contains "zlo"`.
//! Parser iz toga izvuče tekst i vrstu sadržaja, a `rustiio-library` odradi pretragu —
//! CDS ne zna za SQL.

use rustiio_library::store::items;
use rustiio_library::store::search as fts;
use rustiio_library::{Catalog, ItemRow, Node, NodeKind, Store};

use crate::browse::{BrowseOptions, BrowseOutcome, CdsError, node_to_object};

/// Koliko objekata najviše vraćamo u Search odgovoru.
pub const MAX_SEARCH_RESULTS: usize = 500;

/// Što je uređaj tražio.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Criteria {
    /// Tekstovi iz navodnika (ili ostatak kriterija ako navodnika nema).
    pub phrases: Vec<String>,
    /// Kriterij ih veže s `or` (inače `and`).
    pub any_of: bool,
    /// Ograničenje vrste: `video`, `audio`, `image`, `folder`.
    pub kind: Option<&'static str>,
    /// `@refID = "5"` — uređaj traži točno te objekte.
    pub ref_ids: Vec<String>,
    /// `*` — sve iz biblioteke.
    pub everything: bool,
}

/// Zahtjev za pretragu (SOAP argumenti `Search` akcije).
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub container_id: String,
    pub criteria: String,
    pub filter: String,
    pub starting_index: u32,
    pub requested_count: u32,
    pub sort_criteria: String,
}

/// Pretvori UPnP `SearchCriteria` u ono što znamo odgovoriti.
pub fn parse_criteria(input: &str) -> Criteria {
    let text = input.trim();
    let mut criteria = Criteria::default();
    if text.is_empty() || text == "*" {
        criteria.everything = true;
        return criteria;
    }

    let lower = text.to_lowercase();
    criteria.kind = if lower.contains("videoitem") {
        Some("video")
    } else if lower.contains("audioitem") {
        Some("audio")
    } else if lower.contains("imageitem") {
        Some("image")
    } else if lower.contains("object.container") {
        Some("folder")
    } else {
        None
    };
    criteria.any_of = lower.contains(" or ");
    criteria.ref_ids = ref_ids(text);

    criteria.phrases = quoted_phrases(text);
    if criteria.phrases.is_empty() && criteria.ref_ids.is_empty() {
        let loose = strip_operators(text);
        if !loose.is_empty() {
            criteria.phrases.push(loose);
        }
    }
    // Bez teksta za pretragu ne ostaje ništa za filtriranje osim vrste — tada
    // vrijedi "sve iz biblioteke" (npr. `upnp:class derivedfrom "...videoItem"`).
    if criteria.phrases.is_empty() {
        criteria.everything = true;
    }
    criteria
}

/// Vrijednosti iz `@refID = "5"` / `@id = "5"`.
fn ref_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let lower = text.to_lowercase();
    for field in ["@refid", "@id"] {
        let mut search_from = 0;
        while let Some(found) = lower[search_from..].find(field) {
            let position = search_from + found;
            search_from = position + field.len();
            let rest = &text[search_from..];
            if let Some(quoted) = quoted_in(rest).first() {
                let id = quoted.trim().to_string();
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

/// Svi tekstovi u navodnicima unutar jednog kriterija (`"zlo"` → `zlo`).
fn quoted_in(text: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for character in text.chars() {
        match character {
            '"' => {
                if inside {
                    let phrase = current.trim().to_string();
                    if !phrase.is_empty() {
                        phrases.push(phrase);
                    }
                    current.clear();
                }
                inside = !inside;
            }
            _ if inside => current.push(character),
            _ => {}
        }
    }
    phrases
}

/// Podijeli kriterij na klauzule po `and`/`or` (navodnici se ne diraju).
fn split_clauses(text: &str) -> Vec<String> {
    let mut clauses: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut inside_quote = false;
    let mut token = String::new();

    let push_token = |token: &mut String, current: &mut Vec<String>, clauses: &mut Vec<String>| {
        if token.is_empty() {
            return;
        }
        let word = std::mem::take(token);
        let lower = word.to_lowercase();
        if (lower == "and" || lower == "or") && !current.is_empty() {
            clauses.push(current.join(" "));
            current.clear();
        } else {
            current.push(word);
        }
    };

    for character in text.chars() {
        match character {
            '"' => {
                inside_quote = !inside_quote;
                token.push(character);
            }
            c if c.is_whitespace() && !inside_quote => {
                push_token(&mut token, &mut current, &mut clauses);
            }
            c => token.push(c),
        }
    }
    push_token(&mut token, &mut current, &mut clauses);
    if !current.is_empty() {
        clauses.push(current.join(" "));
    }
    clauses
}

/// Tekstovi za pretragu: samo iz klauzula o naslovu/autoru.
///
/// `upnp:class derivedfrom "object.item.videoItem"` nosi naziv klase, ne pojam koji
/// se traži — kad bi i on ušao u FTS upit, `Search` bi uvijek vraćao prazno.
fn quoted_phrases(text: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    for clause in split_clauses(text) {
        let lower = clause.to_lowercase();
        let is_field_value =
            ["upnp:class", "@refid", "@parentid", "@id"].iter().any(|field| lower.contains(field));
        if is_field_value {
            continue;
        }
        phrases.extend(quoted_in(&clause));
    }
    phrases
}

/// Kad navodnika nema: izbaci operatore i ostavi ono što je uređaj htio naći.
fn strip_operators(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut result = String::new();
    for token in text.split_whitespace() {
        // Vrijednosti polja (klase, refID) su u navodnicima — one nisu pojam za pretragu.
        if token.starts_with('"') {
            continue;
        }
        let normalized = token.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        let is_operator = matches!(
            normalized.as_str(),
            "and" | "or" | "contains" | "doesnotcontain" | "derivedfrom" | "exists" | "true" | "false"
        );
        let is_class_name = normalized.starts_with("object.");
        if is_operator
            || is_class_name
            || normalized.contains(':')
            || lower.contains(&format!("{normalized}contains"))
        {
            continue;
        }
        result.push_str(token);
        result.push(' ');
    }
    result.trim().to_string()
}

/// Pretraži biblioteku i vrati DIDL (isti oblik kao `Browse`).
pub fn search_catalog(
    store: &Store,
    catalog: &Catalog,
    request: &SearchRequest,
    options: &BrowseOptions<'_>,
) -> Result<BrowseOutcome, CdsError> {
    let criteria = parse_criteria(&request.criteria);
    let limit = if request.requested_count == 0 {
        MAX_SEARCH_RESULTS
    } else {
        (request.requested_count as usize).min(MAX_SEARCH_RESULTS)
    };
    let page_start = request.starting_index as usize;

    // `@refID` pita za točno određene objekte (TV tako dohvaća spremljene stavke).
    if !criteria.ref_ids.is_empty() {
        let objects: Vec<_> = criteria
            .ref_ids
            .iter()
            .filter_map(|id| catalog.get(id))
            .skip(page_start)
            .take(limit)
            .map(|node| node_to_object(node, catalog, options))
            .collect();
        return Ok(BrowseOutcome {
            didl: rustiio_upnp::render_didl(&objects),
            returned: objects.len() as u32,
            total: criteria.ref_ids.len() as u32,
            update_id: catalog.update_id,
        });
    }

    let found: Vec<ItemRow> = if criteria.everything {
        match criteria.kind {
            Some(kind) => items::by_kind(store, kind, MAX_SEARCH_RESULTS),
            None => items::recent_all(store, MAX_SEARCH_RESULTS),
        }
        .map_err(|error| CdsError::Index(error.to_string()))?
    } else {
        fts::search_phrases(store, &criteria.phrases, criteria.any_of, criteria.kind, MAX_SEARCH_RESULTS)
            .map_err(|error| CdsError::Index(error.to_string()))?
            .into_iter()
            .map(|hit| hit.item)
            .collect()
    };

    let total = found.len() as u32;
    let objects: Vec<_> = found
        .iter()
        .skip(page_start)
        .take(limit)
        .map(|item| node_to_object(&node_for(item, catalog), catalog, options))
        .collect();

    Ok(BrowseOutcome {
        didl: rustiio_upnp::render_didl(&objects),
        returned: objects.len() as u32,
        total,
        update_id: catalog.update_id,
    })
}

/// Objekt iz baze kao čvor kataloga (katalog je izvor istine za titlove i djecu).
fn node_for(item: &ItemRow, catalog: &Catalog) -> Node {
    if let Some(node) = catalog.get(&item.id.to_string()) {
        return node.clone();
    }
    Node {
        id: item.id.to_string(),
        parent_id: String::new(),
        title: item.title.clone(),
        kind: match item.kind.as_str() {
            "video" => NodeKind::Video,
            "audio" => NodeKind::Audio,
            "image" => NodeKind::Image,
            _ => NodeKind::Container,
        },
        path: item.path.clone(),
        size: item.size,
        modified: None,
        children: Vec::new(),
        subtitle: None,
    }
}

/// Prihvaćamo li ovaj `SearchCriteria` na način da TV ne ostane bez ičega.
///
/// Serviio i TV-i traže `dc:title`, `upnp:class` i `@refID`; ako uređaj zatraži
/// polje koje ne znamo (npr. `upnp:actor`), bolje je vratiti prazno i to reći
/// u `SearchCaps` nego glumiti da smo pretražili.
pub const SEARCH_CAPABILITIES: &str = "dc:title,upnp:class,@refID";

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_library::ScanOptions;
    use rustiio_library::scan::scan;

    fn criteria(text: &str) -> Criteria {
        parse_criteria(text)
    }

    #[test]
    fn parses_class_and_quoted_title() {
        let parsed =
            criteria("upnp:class derivedfrom \"object.item.videoItem\" and dc:title contains \"zlo\"");
        assert_eq!(parsed.kind, Some("video"));
        assert_eq!(parsed.phrases, vec!["zlo".to_string()]);
        assert!(!parsed.any_of);
        assert!(!parsed.everything);
    }

    #[test]
    fn star_means_everything() {
        assert!(criteria("*").everything);
        assert!(criteria("").everything);
    }

    #[test]
    fn or_is_recognized() {
        let parsed = criteria("dc:title contains \"a\" or dc:title contains \"b\"");
        assert!(parsed.any_of);
        assert_eq!(parsed.phrases, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn class_value_is_not_a_search_phrase() {
        // Vrijednost `upnp:class` nosi vrstu, ne pojam — inače Search uvijek vraća prazno.
        let parsed =
            criteria("dc:title contains \"zlo\" and upnp:class derivedfrom \"object.item.videoItem\"");
        assert_eq!(parsed.phrases, vec!["zlo".to_string()]);
        assert_eq!(parsed.kind, Some("video"));
    }

    #[test]
    fn quoted_title_keeps_spaces() {
        let parsed = criteria("dc:title contains \"test film\"");
        assert_eq!(parsed.phrases, vec!["test film".to_string()]);
    }

    #[test]
    fn unquoted_title_still_works() {
        let parsed = criteria("dc:title contains zlo");
        assert_eq!(parsed.phrases, vec!["zlo".to_string()]);
    }

    #[test]
    fn class_only_criteria_means_everything_of_that_kind() {
        let parsed = criteria("upnp:class derivedfrom \"object.item.videoItem\"");
        assert!(parsed.everything, "bez teksta vrijedi sve");
        assert_eq!(parsed.kind, Some("video"));
        assert!(parsed.phrases.is_empty(), "naziv klase nije pojam za pretragu");
    }

    #[test]
    fn ref_id_is_read_from_criteria() {
        let parsed = criteria("upnp:class = \"object.item.videoItem\" and @refID = \"42\"");
        assert_eq!(parsed.ref_ids, vec!["42".to_string()]);
        assert!(parsed.phrases.is_empty());
    }

    #[test]
    fn audio_class_is_recognized() {
        assert_eq!(criteria("upnp:class = \"object.item.audioItem.musicTrack\"").kind, Some("audio"));
        assert_eq!(
            criteria("dc:title contains \"x\" and upnp:class derivedfrom \"object.container\"").kind,
            Some("folder")
        );
    }

    #[test]
    fn search_returns_paginated_didl() {
        let dir = std::env::temp_dir().join(format!("rustiio-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mapa");
        for name in ["Zlo S01E01.mkv", "Zlo S01E02.mkv", "Sicario.mkv"] {
            std::fs::write(dir.join(name), b"x").expect("fajl");
        }
        let options = ScanOptions::new(
            vec![rustiio_core::config::Root {
                label: "Filmovi".to_string(),
                path: dir.clone(),
                kind: rustiio_core::config::RootKind::Video,
            }],
            vec!["mkv".to_string()],
        );
        let mut catalog = scan(&options);
        let store = Store::open_memory().expect("baza");
        rustiio_library::adopt_catalog(&store, &mut catalog);

        let browse_options = BrowseOptions {
            base_url: "http://127.0.0.1:8200",
            max_results: 100,
            views: false,
            recent_limit: 5,
            playback: None,
            art: None,
        };
        let request = SearchRequest {
            container_id: "0".to_string(),
            criteria: "upnp:class derivedfrom \"object.item.videoItem\" and dc:title contains \"zlo\""
                .to_string(),
            filter: "*".to_string(),
            starting_index: 0,
            requested_count: 1,
            sort_criteria: String::new(),
        };
        let outcome = search_catalog(&store, &catalog, &request, &browse_options).expect("pretraga");
        assert_eq!(outcome.total, 2, "obe epizode");
        assert_eq!(outcome.returned, 1, "stranica od jednog");
        assert!(outcome.didl.contains("Zlo S01E01"), "DIDL nosi naslov: {}", outcome.didl);

        let empty = search_catalog(
            &store,
            &catalog,
            &SearchRequest { criteria: "dc:title contains \"nema-ovoga\"".to_string(), ..request.clone() },
            &browse_options,
        )
        .expect("pretraga");
        assert_eq!(empty.returned, 0);
        assert_eq!(empty.total, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
