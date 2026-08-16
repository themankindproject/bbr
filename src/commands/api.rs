//! `bbr api` — raw Bitbucket API passthrough.
use crate::cli::GlobalArgs;
use crate::commands::client;
use crate::error::Result;
use crate::output::Formatter;

pub async fn run(
    g: &GlobalArgs,
    method: &str,
    path: &str,
    data: Option<&str>,
    paginate: bool,
) -> Result<()> {
    let client = client(g)?;
    let http_method = method.parse::<reqwest::Method>().map_err(|_| {
        crate::error::BitbucketError::Other(format!("invalid HTTP method: {method}"))
    })?;

    // Keep the response as a `Value` end-to-end: serializing to a string
    // first and then passing it through the JSON formatter would
    // double-encode it (a JSON string containing escaped JSON).
    let value: serde_json::Value = if paginate {
        let values = client
            .fetch_all_pages::<serde_json::Value>(path, usize::MAX)
            .await?;
        serde_json::Value::Array(values)
    } else {
        client.send(http_method, path, data).await?
    };

    let fmt = Formatter::from_json_flag(true);
    fmt.print(&value, "")
}
