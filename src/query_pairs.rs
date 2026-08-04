//! Mutable query-pair API ([`QueryPairsMut`]), analogous to [`PathSegmentsMut`].

use core::ops::{Deref, DerefMut};

use crate::Url;
use crate::search_params::SearchParams;

/// Mutable view of a URL's query as name/value pairs.
///
/// On drop (or [`Self::finish`]), the pairs are serialized back into the URL's
/// query. Empty lists clear the query.
///
/// # Examples
///
/// ```
/// use sorug::Url;
///
/// let mut url = Url::parse("https://example.com/path?a=1").unwrap();
/// url.query_pairs_mut().append("b", "2").append("a", "3");
/// assert_eq!(url.as_str(), "https://example.com/path?a=1&b=2&a=3");
/// ```
#[derive(Debug)]
pub struct QueryPairsMut<'m, 'u> {
    url: &'m mut Url<'u>,
    params: SearchParams,
}

pub(crate) fn new<'m, 'u>(url: &'m mut Url<'u>) -> QueryPairsMut<'m, 'u> {
    let params = url.search_params();
    QueryPairsMut { url, params }
}

impl Drop for QueryPairsMut<'_, '_> {
    fn drop(&mut self) {
        self.url.set_search_params(&self.params);
    }
}

impl Deref for QueryPairsMut<'_, '_> {
    type Target = SearchParams;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

impl DerefMut for QueryPairsMut<'_, '_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.params
    }
}

impl QueryPairsMut<'_, '_> {
    /// Append a name/value pair and return `self` for chaining.
    pub fn append(&mut self, name: &str, value: &str) -> &mut Self {
        self.params.append(name, value);
        self
    }

    /// Set `name` to a single `value` (removing prior pairs with that name).
    pub fn set(&mut self, name: &str, value: &str) -> &mut Self {
        self.params.set(name, value);
        self
    }

    /// Remove all pairs with this name.
    pub fn remove(&mut self, name: &str) -> &mut Self {
        let _ = self.params.delete(name);
        self
    }

    /// Clear all pairs (clears the query on drop).
    pub fn clear(&mut self) -> &mut Self {
        self.params.clear();
        self
    }

    /// Serialize pairs into the URL now and consume this guard without a second write on drop.
    pub fn finish(mut self) {
        // Prevent Drop from writing twice with a moved-out empty list.
        let params = core::mem::take(&mut self.params);
        self.url.set_search_params(&params);
        core::mem::forget(self);
    }
}
