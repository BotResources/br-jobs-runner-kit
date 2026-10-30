use http::uri::PathAndQuery;
use http::{HeaderValue, Uri};
use rustls_pki_types::ServerName;

use crate::error::Error;

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(crate) secure: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) host_header: HeaderValue,
    pub(crate) target: PathAndQuery,
    pub(crate) server_name: ServerName<'static>,
}

fn invalid(reason: &str) -> Error {
    Error::InvalidInput(format!("the endpoint {reason}"))
}

impl Endpoint {
    pub(crate) fn parse(text: &str) -> Result<Self, Error> {
        if text.contains('#') {
            return Err(invalid("holds a fragment"));
        }
        let uri: Uri = text
            .parse()
            .map_err(|_| invalid("cannot be read as a URL"))?;
        let secure = match uri.scheme_str() {
            Some("https") => true,
            Some("http") => false,
            _ => return Err(invalid("is not an absolute http or https URL")),
        };
        let authority = uri
            .authority()
            .ok_or_else(|| invalid("has no host"))?
            .clone();
        if authority.as_str().contains('@') {
            return Err(invalid("holds user information"));
        }
        let host = authority.host();
        if host.is_empty() {
            return Err(invalid("has no host"));
        }
        let bare = host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .unwrap_or(host)
            .to_owned();
        let server_name = ServerName::try_from(bare.clone())
            .map_err(|_| invalid("has a host that is neither a name nor an address"))?;
        let port = authority
            .port_u16()
            .unwrap_or(if secure { 443 } else { 80 });
        let host_header = HeaderValue::from_str(authority.as_str())
            .map_err(|_| invalid("has a host that is not a header value"))?;
        let target = uri
            .path_and_query()
            .cloned()
            .unwrap_or_else(|| PathAndQuery::from_static("/"));
        Ok(Self {
            secure,
            host: bare,
            port,
            host_header,
            target,
            server_name,
        })
    }
}
