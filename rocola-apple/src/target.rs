//! The Apple side of the rocola-core seam.

use rocola_core::{Candidate, MusicTarget, SourceTrack};

use crate::AppleError;
use crate::client::AppleClient;

/// The Apple side of the rocola-core seam: a client bound to one storefront.
///
/// Pure delegation — every decision the matching pipeline makes lives in
/// rocola-core, and every wire detail lives in [`AppleClient`].
pub struct AppleTarget {
    pub client: AppleClient,
    pub storefront: String,
}

impl MusicTarget for AppleTarget {
    type Error = AppleError;

    async fn resolve_by_isrc(
        &self,
        isrcs: &[String],
    ) -> Result<Vec<(String, Candidate)>, AppleError> {
        self.client.resolve_by_isrc(&self.storefront, isrcs).await
    }

    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, AppleError> {
        self.client.search(&self.storefront, track).await
    }
}
