//! Slaganje medija u čitljivo stablo: serije → sezone → epizode, filmovi po abecedi.
//!
//! Zašto ovdje: imena s trackera (`furious.s01e01.1080p.cakes[EZTVx.to]`) i mape koje
//! ih drže (`www.UIndex.org - Dark Matter S02E05 1080p WEBRip …`) daju razbacanu
//! biblioteku. Televizor i web prikazuju **isti** katalog, pa se red slaže jednom, pri
//! skenu, i oba vide isto.
//!
//! Radna pravila:
//! - epizoda se prepoznaje po imenu datoteke; ako u imenu nema naziva serije, uzima se
//!   iz najbliže mape iznad (`Sezona 2/03.mkv`);
//! - sezona koja nije nigdje navedena ide u `Specijali` (S00), kao i u drugim alatima;
//! - prazne mape koje su ostale nakon premještanja se brišu (inače ostanu duhovi);
//! - filmovi se samo ljepše zovu i sortiraju — ne mijenja im se mjesto u stablu.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::naming::{self, ParsedName};
use crate::scan::{Catalog, Node, NodeKind};

/// Što je slaganje promijenilo.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ArrangeSummary {
    pub series: usize,
    pub seasons: usize,
    pub episodes: usize,
    pub movies: usize,
}

/// Je li čvor izmišljen (nije na disku) — prepoznaje se po prefiksu id-a.
pub fn is_synthetic(node: &Node) -> bool {
    node.id.starts_with("s:")
}

/// ID serije/sezone: `s:dark-matter`, `s:dark-matter:2`.
fn series_id(slug: &str) -> String {
    format!("s:{slug}")
}

fn season_id(slug: &str, season: u32) -> String {
    format!("s:{slug}:{season}")
}

/// Naslov sezone — `Specijali` kad sezona nije poznata.
pub fn season_title(season: u32) -> String {
    if season == 0 { "Specijali".to_string() } else { format!("Sezona {season}") }
}

struct Episode {
    node_id: String,
    title: String,
    number: u32,
}

struct Group {
    title: String,
    seasons: BTreeMap<u32, Vec<Episode>>,
}

/// Posloži jedan video korijen: epizode u serije/sezone, filmovi po abecedi.
pub fn arrange_video(catalog: &mut Catalog, root_id: &str, pruni_prazne: bool) -> ArrangeSummary {
    let mut summary = ArrangeSummary::default();

    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    let mut movies: Vec<String> = Vec::new();
    let mut lost_children: HashSet<String> = HashSet::new();

    for node in descendants(catalog, root_id) {
        if node.kind != NodeKind::Video {
            continue;
        }
        let parsed = naming::parse(&node.file_name());
        let context = context_of(catalog, &node, &parsed);

        // Epizoda je ako to ime kaže, ili ako je mapa iznad sezona (`Sezona 2/03.mkv`,
        // `... S02E05 .../epizoda.mkv`) — inače bi svaka epizoda bez oznake ispala film.
        let is_episode = parsed.is_episode() || context.season.is_some();
        if !is_episode {
            if let Some(movie) = catalog.get_mut(&node.id) {
                movie.title = movie_title(&parsed, &node.file_name());
            }
            // Film ostaje gdje jest: u korijen idu samo filmovi koji su izravno u korijenu,
            // inače bismo korisniku razbili vlastite mape (žanrovi, kolekcije…).
            if node.parent_id == root_id {
                movies.push(node.id.clone());
            }
            continue;
        }

        // Seriju imenujemo po vlastitom imenu datoteke, ali kad identitet dolazi iz mape
        // (`Sezona 2/03.mkv`, `... S02E05 .../epizoda.mkv`), vjerujemo mapi.
        let own_identity = parsed.season.is_some() || parsed.episode.is_some();
        let series_title = if own_identity && !parsed.title.is_empty() {
            parsed.title.clone()
        } else if !context.title.is_empty() {
            context.title.clone()
        } else {
            parsed.title.clone()
        };
        let season = parsed.season.or(context.season).unwrap_or(0);
        let number = parsed
            .episode
            .or_else(|| naming::episode_number_hint(&node.file_name()))
            .or(context.episode)
            .unwrap_or(0);
        let slug = slug_of(&series_title);

        let group = groups
            .entry(slug.clone())
            .or_insert_with(|| Group { title: series_title, seasons: BTreeMap::new() });
        // Naslov serije: uzmi najdulji (najpotpuniji) među inačicama imena.
        let title = parsed.title.trim();
        if !title.is_empty() && title.len() > group.title.len() {
            group.title = title.to_string();
        }
        group.seasons.entry(season).or_default().push(Episode {
            node_id: node.id.clone(),
            title: episode_title(season, number, parsed.quality.as_deref()),
            number,
        });
        lost_children.insert(node.parent_id.clone());
    }

    // 1) Presloži epizode pod seriju i sezonu.
    // Uz to, makni ih iz popisa djece starih mapa — inače mapa "izgleda puna",
    // ostane u stablu i nikad se ne očisti.
    let mut orphans: HashMap<String, Vec<String>> = HashMap::new();
    for (slug, group) in &groups {
        let series = series_id(slug);
        for (season, episodes) in &group.seasons {
            let season_node = season_id(slug, *season);
            let mut sorted = episodes.iter().map(|e| e.node_id.clone()).collect::<Vec<_>>();
            let numbers: HashMap<&str, u32> =
                episodes.iter().map(|e| (e.node_id.as_str(), e.number)).collect();
            sorted.sort_by_key(|id| (numbers.get(id.as_str()).copied().unwrap_or(0), id.clone()));

            for episode in episodes {
                if let Some(node) = catalog.get_mut(&episode.node_id) {
                    // Samo prave mape treba čistiti: izmišljenim čvorovima djecu
                    // upisujemo iznova, pa bi ih ovaj prolaz pobrisao.
                    if !node.parent_id.starts_with("s:") {
                        orphans.entry(node.parent_id.clone()).or_default().push(node.id.clone());
                    }
                    node.parent_id = season_node.clone();
                    node.title = episode.title.clone();
                }
            }
            catalog.insert(Node {
                id: season_node.clone(),
                parent_id: series.clone(),
                title: season_title(*season),
                kind: NodeKind::Container,
                path: std::path::PathBuf::new(),
                size: 0,
                modified: None,
                children: sorted,
                subtitles: Vec::new(),
            });
            summary.seasons += 1;
        }

        let seasons: Vec<String> = group.seasons.keys().map(|season| season_id(slug, *season)).collect();
        catalog.insert(Node {
            id: series.clone(),
            parent_id: root_id.to_string(),
            title: group.title.clone(),
            kind: NodeKind::Container,
            path: std::path::PathBuf::new(),
            size: 0,
            modified: None,
            children: seasons,
            subtitles: Vec::new(),
        });
        summary.series += 1;
        summary.episodes += group.seasons.values().map(|episodes| episodes.len()).sum::<usize>();
    }

    for (parent, moved) in &orphans {
        let Some(node) = catalog.get_mut(parent) else { continue };
        node.children.retain(|child| !moved.contains(child));
    }

    // 2) Očisti mape bez videa (i njihove pretke). Kod mješovitog korijena mapa s
    // glazbom nije smeće, pa se tamo ne dira.
    if pruni_prazne {
        prune_empty(catalog, root_id, &lost_children);
    }

    // 3) Slaganje djece korijena: serije, filmovi, pa ostale mape i datoteke.
    // Serije čitamo iz karte (tek su umetnute), a ne iz djece korijena — korijen još
    // nosi stari popis iz skena.
    let mut series_nodes: Vec<Node> =
        groups.keys().filter_map(|slug| catalog.get(&series_id(slug)).cloned()).collect();
    let mut movie_nodes: Vec<Node> = movies.iter().filter_map(|id| catalog.get(id).cloned()).collect();
    let mut rest: Vec<Node> = catalog
        .children(root_id)
        .iter()
        .filter(|node| !series_nodes.iter().any(|series| series.id == node.id))
        .filter(|node| !movies.contains(&node.id))
        .cloned()
        .collect();

    series_nodes.sort_by_key(|node| node.title.to_lowercase());
    movie_nodes.sort_by_key(|node| node.title.to_lowercase());
    rest.sort_by_key(|node| (node.kind != NodeKind::Container, node.title.to_lowercase()));

    let ordered: Vec<String> =
        series_nodes.iter().chain(&movie_nodes).chain(&rest).map(|node| node.id.clone()).collect();
    if let Some(node) = catalog.get_mut(root_id) {
        node.children = ordered;
    }
    summary.movies = movie_nodes.len();

    // 4) Ostale mape (i sve dublje): mape prvo, pa datoteke, abecedno — da i unutar
    // mapa vlada isti red kao u korijenu.
    let containers: Vec<String> = descendants(catalog, root_id)
        .into_iter()
        .filter(|node| node.kind == NodeKind::Container && !is_synthetic(node))
        .map(|node| node.id)
        .collect();
    for id in containers {
        let mut children = catalog.children(&id);
        children.sort_by_key(|node| (node.kind != NodeKind::Container, node.title.to_lowercase()));
        let ids: Vec<String> = children.iter().map(|node| node.id.clone()).collect();
        if let Some(node) = catalog.get_mut(&id) {
            node.children = ids;
        }
    }

    summary
}

/// Posloži korijen bez serija (glazba, slike): mape prvo, pa datoteke, abecedno.
pub fn sort_only(catalog: &mut Catalog, root_id: &str) -> usize {
    let mut children = catalog.children(root_id);
    children.sort_by_key(|node| (node.kind != NodeKind::Container, node.title.to_lowercase()));
    let ids: Vec<String> = children.iter().map(|node| node.id.clone()).collect();
    if let Some(node) = catalog.get_mut(root_id) {
        node.children = ids.clone();
    }
    // I unutar svake mape isti red — inače TV vidi stare datoteke prije novih.
    for child in children.iter().filter(|node| node.kind == NodeKind::Container && !is_synthetic(node)) {
        let id = child.id.clone();
        let mut inner = catalog.children(&id);
        if inner.is_empty() {
            continue;
        }
        inner.sort_by_key(|node| (node.kind != NodeKind::Container, node.title.to_lowercase()));
        let inner_ids: Vec<String> = inner.iter().map(|node| node.id.clone()).collect();
        if let Some(node) = catalog.get_mut(&id) {
            node.children = inner_ids;
        }
    }
    ids.len()
}

/// Posloži korijen prema vrsti: video (i miješano) dobiva serije/sezone/epizode,
/// ostalo samo uredan red (mape pa datoteke, abecedno).
pub fn arrange(catalog: &mut Catalog, root_id: &str, kind: rustiio_core::config::RootKind) -> ArrangeSummary {
    match kind {
        rustiio_core::config::RootKind::Video => arrange_video(catalog, root_id, true),
        // Mješoviti korijen: mape bez videa mogu biti glazba — ne brišu se.
        rustiio_core::config::RootKind::Mixed => arrange_video(catalog, root_id, false),
        _ => {
            sort_only(catalog, root_id);
            ArrangeSummary::default()
        }
    }
}

/// Svi potomci zadanog čvora (bez njega samog).
fn descendants(catalog: &Catalog, id: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = catalog.children(id).iter().map(|node| node.id.clone()).collect();
    while let Some(current) = stack.pop() {
        if !seen.insert(current.clone()) {
            continue; // isti čvor se ne broji dvaput (obrana od petlji u stablu)
        }
        let Some(node) = catalog.get(&current) else { continue };
        out.push(node.clone());
        for child in &node.children {
            stack.push(child.clone());
        }
    }
    out
}

/// Naziv serije, sezona i epizoda iz mapa iznad datoteke (`Zlo/Sezona 2/03.mkv`).
struct Context {
    title: String,
    season: Option<u32>,
    episode: Option<u32>,
}

fn context_of(catalog: &Catalog, node: &Node, parsed: &ParsedName) -> Context {
    let mut context = Context { title: String::new(), season: None, episode: None };
    let mut parent = catalog.get(&node.parent_id);
    let mut hops = 0;

    while let Some(current) = parent {
        hops += 1;
        if hops > 6 {
            break;
        }
        // Već složeno stablo (drugi sken): sezona i serija su izmišljeni čvorovi, a
        // podatke nose u svom id-u (`s:slug:2`) i naslovu.
        if is_synthetic(current) {
            match current.id.rsplit_once(':').and_then(|(_, tail)| tail.parse::<u32>().ok()) {
                Some(season) => {
                    if context.season.is_none() {
                        context.season = Some(season);
                    }
                }
                None => {
                    if context.title.is_empty() {
                        context.title = current.title.clone();
                    }
                }
            }
            parent = catalog.get(&current.parent_id);
            continue;
        }
        if current.path.as_os_str().is_empty() {
            break;
        }
        let from_folder = naming::parse(&current.title);
        if context.title.is_empty() && !from_folder.title.is_empty() {
            context.title = from_folder.title.clone();
        }
        if context.season.is_none() {
            context.season = from_folder.season;
        }
        if context.episode.is_none() {
            context.episode = from_folder.episode;
        }
        if !context.title.is_empty() && context.season.is_some() {
            break;
        }
        parent = catalog.get(&current.parent_id);
    }

    // Ako ni mapa ne pomaže, uzmi njezino ime (očišćeno od smeća) kao naziv serije.
    if context.title.is_empty() {
        if let Some(parent) = catalog.get(&node.parent_id) {
            let cleaned = naming::parse(&parent.title);
            context.title = if cleaned.title.is_empty() { parent.title.clone() } else { cleaned.title };
        }
    }
    if parsed.season.is_some() {
        context.season = parsed.season;
    }
    context
}

/// Oznaka epizode iz **razriješenih** podataka (`S02E05 · 1080p`).
///
/// Ne iz `ParsedName::display()`, jer ono za datoteku bez oznake (`03.mkv`) daje samo
/// broj — a drugi sken bi tako izgubio već složenu oznaku.
fn episode_title(season: u32, number: u32, quality: Option<&str>) -> String {
    let label = if number > 0 { format!("S{season:02}E{number:02}") } else { format!("S{season:02}") };
    match quality {
        Some(quality) => format!("{label} · {quality}"),
        None => label,
    }
}

/// Naslov filma za prikaz: `Zadnji Film (2024)`, a ako ime ne da ništa — ostaje kako je.
fn movie_title(parsed: &ParsedName, fallback: &str) -> String {
    if parsed.title.is_empty() {
        return fallback.to_string();
    }
    match parsed.year {
        Some(year) => format!("{} ({year})", parsed.title),
        None => parsed.title.clone(),
    }
}

/// Mape bez ijednog videa se brišu — takve nisu dio biblioteke.
///
/// Prije se brisalo samo mape koje su ostale *prazne* nakon premještanja epizoda,
/// pa je mapa čiji je jedini sadržaj prazna `Screens` (release smeće) ostajala u
/// korijenu kao kartica s imenom torrenta.
fn prune_empty(catalog: &mut Catalog, root_id: &str, _lost: &HashSet<String>) {
    let mape: Vec<String> = descendants(catalog, root_id)
        .into_iter()
        .filter(|node| node.kind == NodeKind::Container && !is_synthetic(node))
        .map(|node| node.id.clone())
        .collect();

    // Gleda se stanje **prije** brisanja, pa roditelj nestaje zajedno s djetetom.
    let removed: HashSet<String> = mape.into_iter().filter(|id| !has_video(catalog, id)).collect();

    for id in &removed {
        catalog.remove(id);
    }
    if !removed.is_empty() {
        detach(catalog, root_id, &removed);
    }
}

/// Ima li u podstablu ijedan video (ni sam čvor se ne računa ako nije video).
fn has_video(catalog: &Catalog, id: &str) -> bool {
    let mut red: Vec<String> = vec![id.to_string()];
    let mut videno: HashSet<String> = HashSet::new();
    while let Some(trenutni) = red.pop() {
        if !videno.insert(trenutni.clone()) {
            continue;
        }
        let Some(node) = catalog.get(&trenutni) else { continue };
        if node.kind == NodeKind::Video {
            return true;
        }
        red.extend(node.children.iter().cloned());
    }
    false
}

/// Makni obrisane čvorove iz popisa djece svih roditelja.
fn detach(catalog: &mut Catalog, root_id: &str, gone: &HashSet<String>) {
    let mut ids: Vec<String> = vec![root_id.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(id) = ids.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = catalog.get(&id) else { continue };
        let children: Vec<String> = node.children.clone();
        let kept: Vec<String> = children.iter().filter(|child| !gone.contains(*child)).cloned().collect();
        if kept.len() != children.len() {
            if let Some(node) = catalog.get_mut(&id) {
                node.children = kept;
            }
        }
        ids.extend(children.into_iter().filter(|child| !gone.contains(child)));
    }
}

/// `dark matter` → `dark-matter` (za ID izmišljenih čvorova).
fn slug_of(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut dash = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanOptions;
    use crate::scan::scan;
    use rustiio_core::config::{Root, RootKind};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = format!(
            "rustiio-grouping-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("mapa");
        path
    }

    fn write(dir: &std::path::Path, relative: &str) {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("podmapa");
        }
        std::fs::write(path, b"x").expect("datoteka");
    }

    fn catalog_for(dir: &std::path::Path, extensions: &[&str]) -> Catalog {
        catalog_for_kind(dir, extensions, RootKind::Video)
    }

    fn catalog_for_kind(dir: &std::path::Path, extensions: &[&str], kind: RootKind) -> Catalog {
        let options = ScanOptions::new(
            vec![Root { label: "Serije".to_string(), path: dir.to_path_buf(), kind }],
            extensions.iter().map(|ext| ext.to_string()).collect(),
        );
        scan(&options)
    }

    fn root_id(catalog: &Catalog) -> String {
        catalog.top_level().first().expect("korijen").id.clone()
    }

    fn titles(catalog: &Catalog, id: &str) -> Vec<String> {
        catalog.children(id).iter().map(|node| node.title.clone()).collect()
    }

    #[test]
    fn scattered_episodes_become_series_season_episode() {
        let dir = temp_dir("serije");
        // Razbacano kako stvarno dolazi s trackera: mape s imenom izdanja u raznim oblicima.
        write(&dir, "furious.s01e02.1080p.cakes[EZTVx.to].mkv");
        write(&dir, "furious.s01e01.1080p.cakes[EZTVx.to].mkv");
        write(&dir, "www.UIndex.org - Dark Matter S02E05 1080p WEBRip 10Bit DDP5 1 HEVC-d3g/epizoda.mkv");
        write(
            &dir,
            "www.UIndex.org - Dark Matter S02E05 1080p WEBRip 10Bit DDP5 1 HEVC-d3g/Dark.Matter.2024.S02E05.1080p.WEB-DL.mkv",
        );
        write(&dir, "The.Bureau.S01.10bit.x265/The.Bureau.S01E01.mkv");
        write(&dir, "The.Bureau.S01.10bit.x265/The.Bureau.S01E02.mkv");

        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        let summary = arrange_video(&mut catalog, &root, true);

        assert_eq!(summary.series, 3, "Furious, Dark Matter, The Bureau");
        assert_eq!(titles(&catalog, &root), vec!["Dark Matter", "Furious", "The Bureau"], "serije abecedno");

        // Furious: jedna sezona, epizode po redu.
        let furious =
            catalog.children(&root).into_iter().find(|node| node.title == "Furious").expect("Furious");
        assert_eq!(titles(&catalog, &furious.id), vec!["Sezona 1"]);
        let season = catalog.children(&furious.id).remove(0);
        assert_eq!(titles(&catalog, &season.id), vec!["S01E01 · 1080p", "S01E02 · 1080p"]);

        // Dark Matter: epizoda iz mape s iščupanim imenom ipak ide pod seriju i sezonu.
        let dark = catalog
            .children(&root)
            .into_iter()
            .find(|node| node.title == "Dark Matter")
            .expect("Dark Matter");
        let season = catalog.children(&dark.id).remove(0);
        assert_eq!(season.title, "Sezona 2");
        let episodes = catalog.children(&season.id);
        assert_eq!(episodes.len(), 2, "obje datoteke iz te mape");

        // Drugi sken (server skenira u krug) ne smije ništa pokvariti.
        let before = titles(&catalog, &root);
        let series_before = titles(&catalog, &dark.id);
        let season_before = titles(&catalog, &season.id);
        arrange_video(&mut catalog, &root, true);
        assert_eq!(titles(&catalog, &root), before, "korijen ostaje isti");
        assert_eq!(titles(&catalog, &dark.id), series_before, "serija ostaje ista");
        assert_eq!(titles(&catalog, &season.id), season_before, "epizode ostaju iste");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_folders_left_behind_are_removed() {
        let dir = temp_dir("prazne");
        write(&dir, "OvdjeJeBilaSerija S01E01 1080p/S01E01.mkv");
        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        arrange_video(&mut catalog, &root, true);

        let root_children = catalog.children(&root);
        assert!(
            !root_children.iter().any(|node| node.title.contains("S01E01")),
            "mapa s imenom izdanja je obrisana: {:?}",
            root_children.iter().map(|node| node.title.clone()).collect::<Vec<_>>()
        );
        assert_eq!(root_children.len(), 1);
        assert_eq!(root_children[0].title, "OvdjeJeBilaSerija");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn season_from_folder_above_and_episode_from_number() {
        let dir = temp_dir("sezona");
        write(&dir, "Zlo/Sezona 2/03.mkv");
        write(&dir, "Zlo/Sezona 2/01.mkv");
        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        arrange_video(&mut catalog, &root, true);

        let zlo = catalog.children(&root).remove(0);
        assert_eq!(zlo.title, "Zlo");
        let season = catalog.children(&zlo.id).remove(0);
        assert_eq!(season.title, "Sezona 2");
        assert_eq!(titles(&catalog, &season.id), vec!["S02E01", "S02E03"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_season_goes_to_specials() {
        let dir = temp_dir("specijali");
        write(&dir, "Neka Serija E05 1080p.mkv");
        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        arrange_video(&mut catalog, &root, true);

        let series = catalog.children(&root).remove(0);
        assert_eq!(series.title, "Neka Serija", "naziv i sezona se čitaju iz imena datoteke");
        assert_eq!(titles(&catalog, &series.id), vec!["Specijali"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn movies_stay_and_get_sorted_alphabetically() {
        let dir = temp_dir("filmovi");
        write(&dir, "Zadnji.Film.2024.1080p.WEB-DL.mkv");
        write(&dir, "Abeceda.2020.1080p.BluRay.mkv");
        write(&dir, "Podmapa/Nešto.mkv");
        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        let summary = arrange_video(&mut catalog, &root, true);

        assert_eq!(summary.movies, 2, "dva filma u korijenu; onaj u podmapi ostaje u svojoj mapi");
        let titles = titles(&catalog, &root);
        assert_eq!(
            titles,
            vec!["Abeceda (2020)", "Zadnji Film (2024)", "Podmapa"],
            "filmovi abecedno i s godinom, mape na kraju"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sorting_only_works_for_other_roots() {
        let dir = temp_dir("sort");
        write(&dir, "b/Song.mp3");
        write(&dir, "a/Song.mp3");
        let mut catalog = catalog_for_kind(&dir, &["mkv", "mp3"], RootKind::Mixed);
        let root = root_id(&catalog);
        sort_only(&mut catalog, &root);
        assert_eq!(titles(&catalog, &root), vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_with_only_junk_subfolders_is_removed() {
        // Točno slučaj s boxa: epizoda se preselila u seriju, a u mapi je ostala
        // samo prazna `Screens`. Mapa ne smije ostati kao kartica u korijenu.
        let dir = temp_dir("screens");
        write(&dir, "www.UIndex.org - Slow.Horses.S06E02.1080p/Screens/slika.jpg");
        write(&dir, "www.UIndex.org - Slow.Horses.S06E02.1080p/slow.horses.s06e02.1080p.mkv");
        let mut catalog = catalog_for(&dir, &["mkv"]);
        let root = root_id(&catalog);
        arrange_video(&mut catalog, &root, true);
        assert_eq!(titles(&catalog, &root), vec!["Slow Horses"], "ostala je mapa smeća");
        assert_eq!(catalog.children("s:slow-horses").len(), 1, "sezona mora biti jedna");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mixed_root_keeps_folders_without_video() {
        // Glazba u mješovitom korijenu nije smeće — mape se ne brišu.
        let dir = temp_dir("mixed");
        write(&dir, "Glazba/album/pjesma.mp3");
        let mut catalog = catalog_for_kind(&dir, &["mkv", "mp3"], RootKind::Mixed);
        let root = root_id(&catalog);
        arrange_video(&mut catalog, &root, false);
        assert_eq!(titles(&catalog, &root), vec!["Glazba"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
