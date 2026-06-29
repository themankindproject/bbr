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

    let raw = if paginate {
        let values = client
            .fetch_all_pages::<serde_json::Value>(path, usize::MAX)
            .await?;
        serde_json::to_string_pretty(&values)?
    } else {
        if http_method == reqwest::Method::GET && data.is_none() {
            let val: serde_json::Value = client.send(http_method, path, None).await?;
            serde_json::to_string_pretty(&val)?
        } else {
            let val: serde_json::Value = client.send(http_method, path, data).await?;
            serde_json::to_string_pretty(&val)?
        }
    };

    let fmt = Formatter::from_json_flag(true);
    fmt.print(&raw, &raw)
}
