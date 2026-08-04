//! Cursor pagination — mirrors the TypeScript SDK's `SeekPage<T>` + `listAll`.

use serde::Deserialize;

use crate::Error;

/// One page of a cursor-paginated list.
///
/// `next` / `previous` are opaque cursors; pass `next` back to the same
/// endpoint to fetch the following page. A `null`/absent `next` means the
/// walk is complete.
#[derive(Debug, Clone, Deserialize)]
pub struct SeekPage<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub previous: Option<String>,
}

impl<T> SeekPage<T> {
    /// True when there are no more pages after this one.
    pub fn is_last(&self) -> bool {
        self.next.is_none()
    }
}

/// Walk every page of a cursor-paginated endpoint into a single `Vec`.
///
/// `fetch` is called with `None` for the first page, then the previous page's
/// `next` cursor until it comes back empty. Resource methods wrap their paged
/// endpoint in a closure and hand it here:
///
/// ```no_run
/// # use memmesh::{list_all, SeekPage, Error};
/// # async fn run(http: impl Fn(Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<SeekPage<String>, Error>>>>) -> Result<(), Error> {
/// let all = list_all(|cursor| http(cursor)).await?;
/// # let _ = all; Ok(()) }
/// ```
pub async fn list_all<T, F, Fut>(mut fetch: F) -> Result<Vec<T>, Error>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<SeekPage<T>, Error>>,
{
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = fetch(cursor).await?;
        out.extend(page.data);
        match page.next {
            Some(next) => cursor = Some(next),
            None => return Ok(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_page_deserializes_and_defaults() {
        let p: SeekPage<i32> = serde_json::from_str(r#"{"data":[1,2],"next":"c1"}"#).unwrap();
        assert_eq!(p.data, vec![1, 2]);
        assert_eq!(p.next.as_deref(), Some("c1"));
        assert!(p.previous.is_none());
        assert!(!p.is_last());

        // Missing `data` defaults to empty; missing `next` = last page.
        let empty: SeekPage<i32> = serde_json::from_str("{}").unwrap();
        assert!(empty.data.is_empty());
        assert!(empty.is_last());
    }

    #[tokio::test]
    async fn list_all_walks_every_page() {
        // Three pages: cursors c1 -> c2 -> end.
        let pages = [
            SeekPage {
                data: vec![1, 2],
                next: Some("c1".into()),
                previous: None,
            },
            SeekPage {
                data: vec![3, 4],
                next: Some("c2".into()),
                previous: Some("c0".into()),
            },
            SeekPage {
                data: vec![5],
                next: None,
                previous: Some("c1".into()),
            },
        ];
        let mut idx = 0usize;
        let mut seen_cursors: Vec<Option<String>> = Vec::new();

        let all = list_all(|cursor: Option<String>| {
            seen_cursors.push(cursor);
            let page = SeekPage {
                data: pages[idx].data.clone(),
                next: pages[idx].next.clone(),
                previous: pages[idx].previous.clone(),
            };
            idx += 1;
            async move { Ok::<_, Error>(page) }
        })
        .await
        .unwrap();

        assert_eq!(all, vec![1, 2, 3, 4, 5]);
        // First call gets None, then the prior page's `next`.
        assert_eq!(
            seen_cursors,
            vec![None, Some("c1".to_string()), Some("c2".to_string())]
        );
    }

    #[tokio::test]
    async fn list_all_propagates_errors() {
        let result: Result<Vec<i32>, Error> =
            list_all(|_| async { Err(Error::Timeout) }).await;
        assert!(matches!(result, Err(Error::Timeout)));
    }
}
