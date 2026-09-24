//! SOAP: sta TV posalje u `POST /ContentDirectory/control` i sta mu vratimo.
//!
//! Akciju citamo iz `SOAPACTION` headera (tako rade svi), a argumente iz tijela.
//! Argumenti su uvijek plitki (`<ObjectID>0</ObjectID>`), pa je i parser plitak.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::escape;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `urn:schemas-upnp-org:service:ContentDirectory:1`
    pub service: String,
    /// `Browse`
    pub action: String,
}

/// Parsiraj `SOAPACTION: "urn:...:service:ContentDirectory:1#Browse"`.
pub fn parse_action_header(value: &str) -> Option<Request> {
    let trimmed = value.trim().trim_matches('"');
    let (service, action) = trimmed.split_once('#')?;
    if service.is_empty() || action.is_empty() {
        return None;
    }
    Some(Request { service: service.to_string(), action: action.to_string() })
}

/// Izvuci argumente iz SOAP tijela: ime elementa -> tekst.
///
/// Vrijednost se skuplja u buffer dok se element ne zatvori — tako radi i kad je
/// tekst razlomljen entitetima (`&amp;` quick-xml salje kao `GeneralRef`, ne kao text).
pub fn parse_args(body: &str) -> HashMap<String, String> {
    let mut reader = Reader::from_str(body);
    let mut args = HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                current = Some(local_name(start.name().as_ref()));
                buffer.clear();
            }
            Ok(Event::Text(text)) => {
                if current.is_some() {
                    if let Ok(value) = text.decode() {
                        buffer.push_str(&value);
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if current.is_some() {
                    match reference.resolve_char_ref() {
                        Ok(Some(ch)) => buffer.push(ch),
                        Ok(None) => {
                            if let Ok(name) = reference.decode() {
                                match name.as_ref() {
                                    "amp" => buffer.push('&'),
                                    "lt" => buffer.push('<'),
                                    "gt" => buffer.push('>'),
                                    "quot" => buffer.push('"'),
                                    "apos" => buffer.push('\''),
                                    _ => {}
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            Ok(Event::End(_)) => {
                if let Some(name) = current.take() {
                    let value = buffer.trim();
                    if !value.is_empty() {
                        args.insert(name, value.to_string());
                    }
                }
                buffer.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    args
}

fn local_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    match text.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => text.to_string(),
    }
}

/// Uspjesan SOAP odgovor s proizvoljnim sadrzajem odgovora akcije.
pub fn response(service: &str, action: &str, inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:{action}Response xmlns:u="{service}">
{inner}
    </u:{action}Response>
  </s:Body>
</s:Envelope>
"#,
        action = action,
        service = service,
        inner = inner
    )
}

/// SOAP Fault s UPnP error kodom (npr. 701 No such object).
pub fn fault(code: u32, description: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <s:Fault>
      <faultcode>s:Client</faultcode>
      <faultstring>UPnPError</faultstring>
      <detail>
        <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
          <errorCode>{code}</errorCode>
          <errorDescription>{description}</errorDescription>
        </UPnPError>
      </detail>
    </s:Fault>
  </s:Body>
</s:Envelope>
"#,
        code = code,
        description = escape(description)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_action_header() {
        let req =
            parse_action_header("\"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"").expect("parse");
        assert_eq!(req.action, "Browse");
        assert_eq!(req.service, "urn:schemas-upnp-org:service:ContentDirectory:1");
        assert!(parse_action_header("nesto-bez-tocke").is_none());
    }

    #[test]
    fn parses_browse_args_from_real_body() {
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
 <s:Body>
  <u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
   <ObjectID>0</ObjectID>
   <BrowseFlag>BrowseDirectChildren</BrowseFlag>
   <Filter>*</Filter>
   <StartingIndex>0</StartingIndex>
   <RequestedCount>0</RequestedCount>
   <SortCriteria>+dc:title</SortCriteria>
  </u:Browse>
 </s:Body>
</s:Envelope>"#;
        let args = parse_args(body);
        assert_eq!(args.get("ObjectID").map(String::as_str), Some("0"));
        assert_eq!(args.get("BrowseFlag").map(String::as_str), Some("BrowseDirectChildren"));
        assert_eq!(args.get("StartingIndex").map(String::as_str), Some("0"));
        assert_eq!(args.get("SortCriteria").map(String::as_str), Some("+dc:title"));
    }

    #[test]
    fn parses_unescaped_entities_and_ignores_empty() {
        let body = "<u:Browse><Filter>&amp;test</Filter><ObjectID>1</ObjectID><Empty></Empty></u:Browse>";
        let args = parse_args(body);
        assert_eq!(args.get("Filter").map(String::as_str), Some("&test"));
        assert!(!args.contains_key("Empty"));
    }

    #[test]
    fn response_and_fault_are_well_formed_envelopes() {
        let ok = response(
            "urn:schemas-upnp-org:service:ConnectionManager:1",
            "GetProtocolInfo",
            "      <Source>http-get:*:video/mp4:*</Source>",
        );
        assert!(ok.contains("<u:GetProtocolInfoResponse"));
        assert!(ok.contains("</s:Envelope>"));

        let err = soap_fault_static();
        assert!(err.contains("<errorCode>701</errorCode>"));
    }

    fn soap_fault_static() -> String {
        fault(701, "No such object")
    }
}
