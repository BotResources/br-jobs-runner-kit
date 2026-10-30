use std::net::Ipv6Addr;

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

fn split(authority: &str) -> Result<(&str, Option<&str>), Error> {
    if let Some(inside) = authority.strip_prefix('[') {
        let (address, rest) = inside
            .split_once(']')
            .ok_or_else(|| invalid("has a host with an opening bracket and no closing one"))?;
        address
            .parse::<Ipv6Addr>()
            .map_err(|_| invalid("has a bracketed host that is not an IPv6 address"))?;
        return match rest {
            "" => Ok((address, None)),
            _ => match rest.strip_prefix(':') {
                Some(digits) => Ok((address, Some(digits))),
                None => Err(invalid(
                    "has text after the bracketed host that is not a port",
                )),
            },
        };
    }
    match authority.rsplit_once(':') {
        Some((host, _)) if host.contains(':') => {
            Err(invalid("has a host with a colon outside brackets"))
        }
        Some((host, digits)) => Ok((host, Some(digits))),
        None => Ok((authority, None)),
    }
}

fn port_of(digits: &str) -> Result<u16, Error> {
    let all_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    match all_digits.then(|| digits.parse::<u16>()) {
        Some(Ok(port)) if port != 0 => Ok(port),
        _ => Err(invalid("has a port that is not a number from 1 to 65535")),
    }
}

fn target_of(uri: &Uri) -> Result<PathAndQuery, Error> {
    let target = match uri.path_and_query().map(PathAndQuery::as_str) {
        None | Some("") => "/".to_owned(),
        Some(text) if text.starts_with('?') => format!("/{text}"),
        Some(text) => text.to_owned(),
    };
    target
        .parse()
        .map_err(|_| invalid("has a path or query that cannot be sent"))
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
        let (host, port_text) = split(authority.as_str())?;
        if host.is_empty() {
            return Err(invalid("has no host"));
        }
        let bare = host.to_owned();
        let server_name = ServerName::try_from(bare.clone())
            .map_err(|_| invalid("has a host that is neither a name nor an address"))?;
        let port = match port_text {
            Some(digits) => port_of(digits)?,
            None if secure => 443,
            None => 80,
        };
        let host_header = HeaderValue::from_str(authority.as_str())
            .map_err(|_| invalid("has a host that is not a header value"))?;
        let target = target_of(&uri)?;
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
