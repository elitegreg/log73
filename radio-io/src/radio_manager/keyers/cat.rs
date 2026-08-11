use super::super::cw_task::{CwKeyer, CwKeyerStatus, CwSendCompletion};
use crate::cat_keyer::CatKeyer;
use futures_util::future::{BoxFuture, FutureExt};
use tracing::debug;

impl CwKeyer for CatKeyer {
    fn name(&self) -> &'static str {
        "CAT"
    }

    fn connect<'a>(&'a mut self, radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            debug!(radio_id, "CAT CW keyer ready");
        }
        .boxed()
    }

    fn send_text<'a>(
        &'a mut self,
        _radio_id: i64,
        text: &'a str,
    ) -> BoxFuture<'a, Result<CwSendCompletion, String>> {
        async move {
            let updates = self.subscribe_updates();
            CatKeyer::send_text(self, text).await?;
            Ok(CwSendCompletion::RadioCatUpdates(updates))
        }
        .boxed()
    }

    fn status<'a>(
        &'a mut self,
        _radio_id: i64,
    ) -> BoxFuture<'a, Result<Option<CwKeyerStatus>, String>> {
        async move { Ok(None) }.boxed()
    }

    fn clear_buffer<'a>(&'a mut self, _radio_id: i64) -> BoxFuture<'a, Result<(), String>> {
        async move { CatKeyer::clear_buffer(self).await }.boxed()
    }

    fn set_wpm<'a>(&'a mut self, _radio_id: i64, wpm: u8) -> BoxFuture<'a, Result<(), String>> {
        async move { CatKeyer::set_wpm(self, wpm).await }.boxed()
    }

    fn close<'a>(&'a mut self, _radio_id: i64) -> BoxFuture<'a, ()> {
        async move {
            CatKeyer::close(self).await;
        }
        .boxed()
    }
}
