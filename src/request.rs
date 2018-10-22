//! API Gateway request types. Typically these are exposed via the `request_context` method provided by [lando::RequestExt](trait.RequestExt.html)

// Std
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::mem;

// Third Party
use http::header::{HeaderValue, HOST};
use http::Request as HttpRequest;
use http::{self, HeaderMap};
use serde::{de::Error as DeError, de::MapAccess, de::Visitor, Deserialize, Deserializer};

use body::Body;
use ext::{PathParameters, QueryStringParameters, StageVariables};

/// Representation of an API Gateway proxy event data
///
/// Note: This should really be pub(crate) but is pub for
/// bench mark testing
#[doc(hidden)]
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRequest<'a> {
    //pub resDeserializeHeadersource: String,
    pub(crate) path: Cow<'a, str>,
    pub(crate) http_method: Cow<'a, str>,
    #[serde(deserialize_with = "deserialize_headers")]
    pub(crate) headers: HeaderMap<HeaderValue>,
    #[serde(deserialize_with = "nullable_default")]
    pub(crate) query_string_parameters: HashMap<String, String>,
    #[serde(deserialize_with = "nullable_default")]
    pub(crate) path_parameters: HashMap<String, String>,
    #[serde(deserialize_with = "nullable_default")]
    pub(crate) stage_variables: HashMap<String, String>,
    pub(crate) body: Option<Cow<'a, str>>,
    #[serde(default)]
    pub(crate) is_base64_encoded: bool,
    pub(crate) request_context: RequestContext,
}

/// API Gateway request context
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    //pub path: String,
    pub account_id: String,
    pub resource_id: String,
    pub stage: String,
    pub request_id: String,
    pub resource_path: String,
    pub http_method: String,
    //pub authorizer: HashMap<String, String>,
    pub api_id: String,
    pub identity: Identity,
}

/// Identity assoicated with request
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub source_ip: String,
    pub cognito_identity_id: Option<String>,
    pub cognito_identity_pool_id: Option<String>,
    pub cognito_authentication_provider: Option<String>,
    pub cognito_authentication_type: Option<String>,
    pub account_id: Option<String>,
    pub caller: Option<String>,
    pub api_key: Option<String>,
    pub user: Option<String>,
    pub user_agent: Option<String>,
    pub user_arn: Option<String>,
}

fn deserialize_headers<'de, D>(deserializer: D) -> Result<HeaderMap<HeaderValue>, D::Error>
where
    D: Deserializer<'de>,
{
    struct HeaderVisitor;

    impl<'de> Visitor<'de> for HeaderVisitor {
        type Value = HeaderMap<HeaderValue>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a HeaderMap<HeaderValue>")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut headers = http::HeaderMap::new();
            while let Some((key, value)) = map.next_entry::<Cow<str>, Cow<str>>()? {
                let header_name = key
                    .parse::<http::header::HeaderName>()
                    .map_err(A::Error::custom)?;
                let header_value =
                    http::header::HeaderValue::from_shared(value.into_owned().into())
                        .map_err(A::Error::custom)?;
                headers.append(header_name, header_value);
            }
            Ok(headers)
        }
    }

    deserializer.deserialize_map(HeaderVisitor)
}

/// deserializes (json) null values to their default values
// https://github.com/serde-rs/serde/issues/1098
fn nullable_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_else(Default::default))
}

impl<'a> From<GatewayRequest<'a>> for HttpRequest<Body> {
    fn from(value: GatewayRequest) -> Self {
        let GatewayRequest {
            path,
            http_method,
            headers,
            query_string_parameters,
            path_parameters,
            stage_variables,
            body,
            is_base64_encoded,
            request_context,
        } = value;

        // build an http::Request<lando::Body> from a lando::GatewayRequest
        let mut builder = HttpRequest::builder();
        builder.method(http_method.as_ref());
        builder.uri({
            format!(
                "https://{}{}",
                headers
                    .get(HOST)
                    .map(|val| val.to_str().unwrap_or_default())
                    .unwrap_or_default(),
                path
            )
        });

        builder.extension(QueryStringParameters(query_string_parameters));
        builder.extension(PathParameters(path_parameters));
        builder.extension(StageVariables(stage_variables));
        builder.extension(request_context);

        let mut req = builder
            .body(match body {
                Some(b) => {
                    if is_base64_encoded {
                        // todo: document failure behavior
                        Body::from(::base64::decode(b.as_ref()).unwrap_or_default())
                    } else {
                        Body::from(b.into_owned())
                    }
                }
                _ => Body::from(()),
            })
            .expect("failed to build request");

        // no builder method that sets headers in batch
        mem::replace(req.headers_mut(), headers);

        req
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    use std::collections::HashMap;

    #[test]
    fn requests_convert() {
        let mut headers = HeaderMap::new();
        headers.insert("Host", "www.rust-lang.org".parse().unwrap());
        let gwr: GatewayRequest = GatewayRequest {
            path: "/foo".into(),
            http_method: "GET".into(),
            headers,
            ..Default::default()
        };
        let expected = HttpRequest::get("https://www.rust-lang.org/foo")
            .body(())
            .unwrap();
        let actual = HttpRequest::from(gwr);
        assert_eq!(expected.uri(), actual.uri());
        assert_eq!(expected.method(), actual.method());
    }

    #[test]
    fn deserializes_request_events() {
        // from the docs
        // https://docs.aws.amazon.com/lambda/latest/dg/eventsources.html#eventsources-api-gateway-request
        let input = include_str!("../tests/data/proxy_request.json");
        assert!(serde_json::from_str::<GatewayRequest>(&input).is_ok())
    }

    #[test]
    fn implements_default() {
        assert_eq!(
            GatewayRequest {
                path: "/foo".into(),
                ..Default::default()
            }
            .path,
            "/foo"
        )
    }

    #[test]
    fn deserialize_with_null() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Test {
            #[serde(deserialize_with = "nullable_default")]
            foo: HashMap<String, String>,
        }

        assert_eq!(
            serde_json::from_str::<Test>(r#"{"foo":null}"#).expect("failed to deserialize"),
            Test {
                foo: HashMap::new(),
            }
        )
    }

}
