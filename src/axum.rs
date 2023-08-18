use std::pin::Pin;
use std::task::Poll;

use axum::response::sse::Event;
use futures::stream::{Stream, StreamExt, TryStream};
use pin_project_lite::pin_project;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
pub use tokio::sync::mpsc::error::{SendError, TrySendError};
use tokio_stream::wrappers::ReceiverStream;

use crate::ServerSignalUpdate;

pin_project! {
    /// A signal owned by the server which writes to the SSE when mutated.
    #[derive(Clone, Debug)]
    pub struct ServerSentEvent<S> {
        #[pin]
        stream: S,
        json_value: Value,
    }
}

impl<S> ServerSentEvent<S> {
    /// Create a new [`ServerSentEvent`] a stream, initializing `T` to default.
    ///
    /// This function can fail if serilization of `T` fails.
    pub fn from_stream<T>(stream: S) -> Result<Self, serde_json::Error>
    where
        T: Default + Serialize,
        S: TryStream<Ok = T, Error = axum::BoxError>,
    {
        Ok(ServerSentEvent {
            stream,
            json_value: serde_json::to_value(T::default())?,
        })
    }

    /// Create a server-sent-events (SSE) channel pair.
    ///
    /// The `buffer` argument controls how many unsent messages can be stored without waiting.
    ///
    /// The first item in the tuple is the MPSC channel sender half.
    pub fn channel<T>(
        buffer: usize,
    ) -> Result<
        (
            Sender<T>,
            ServerSentEvent<impl TryStream<Ok = T, Error = axum::BoxError>>,
        ),
        serde_json::Error,
    >
    where
        T: Default + Serialize,
    {
        let (sender, receiver) = mpsc::channel::<T>(buffer);
        let stream = ReceiverStream::new(receiver).map(Ok);
        Ok((Sender(sender), ServerSentEvent::from_stream(stream)?))
    }
}

impl<S> Stream for ServerSentEvent<S>
where
    S: TryStream<Error = axum::BoxError>,
    S::Ok: Serialize,
{
    type Item = Result<Event, axum::BoxError>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.stream.try_poll_next(cx) {
            Poll::Ready(Some(Ok(value))) => {
                let new_json = serde_json::to_value(value)?;
                let update =
                    ServerSignalUpdate::new_from_json::<S::Item>(this.json_value, &new_json);
                *this.json_value = new_json;
                let event = Event::default().json_data(update)?;
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sender<T>(mpsc::Sender<T>);

impl<T> Sender<T> {
    /// Send an SSE message.
    pub async fn send(&self, value: T) -> Result<(), SendError<T>>
    where
        T: Serialize,
    {
        self.0.send(value).await
    }

    /// Attempts to immediately send an SSE message.
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>>
    where
        T: Serialize,
    {
        self.0.try_send(value)
    }
}
