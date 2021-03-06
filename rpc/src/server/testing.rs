use crate::server::{Channel, Config};
use crate::{context, Request, Response};
use fnv::FnvHashSet;
use futures::future::{AbortHandle, AbortRegistration};
use futures::{Sink, Stream};
use futures_test::task::noop_waker_ref;
use pin_project::pin_project;
use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::SystemTime;

#[pin_project]
pub(crate) struct FakeChannel<In, Out> {
    #[pin]
    pub stream: VecDeque<In>,
    #[pin]
    pub sink: VecDeque<Out>,
    pub config: Config,
    pub in_flight_requests: FnvHashSet<u64>,
}

impl<In, Out> Stream for FakeChannel<In, Out>
where
    In: Unpin,
{
    type Item = In;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}

impl<In, Resp> Sink<Response<Resp>> for FakeChannel<In, Response<Resp>> {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project().sink.poll_ready(cx).map_err(|e| match e {})
    }

    fn start_send(mut self: Pin<&mut Self>, response: Response<Resp>) -> Result<(), Self::Error> {
        self.as_mut()
            .project()
            .in_flight_requests
            .remove(&response.request_id);
        self.project()
            .sink
            .start_send(response)
            .map_err(|e| match e {})
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project().sink.poll_flush(cx).map_err(|e| match e {})
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.project().sink.poll_close(cx).map_err(|e| match e {})
    }
}

impl<Req, Resp> Channel for FakeChannel<io::Result<Request<Req>>, Response<Resp>>
where
    Req: Unpin,
{
    type Req = Req;
    type Resp = Resp;

    fn config(&self) -> &Config {
        &self.config
    }

    fn in_flight_requests(self: Pin<&mut Self>) -> usize {
        self.in_flight_requests.len()
    }

    fn start_request(self: Pin<&mut Self>, id: u64) -> AbortRegistration {
        self.project().in_flight_requests.insert(id);
        AbortHandle::new_pair().1
    }
}

impl<Req, Resp> FakeChannel<io::Result<Request<Req>>, Response<Resp>> {
    pub fn push_req(&mut self, id: u64, message: Req) {
        self.stream.push_back(Ok(Request {
            context: context::Context {
                deadline: SystemTime::UNIX_EPOCH,
                trace_context: Default::default(),
                _non_exhaustive: (),
            },
            id,
            message,
            _non_exhaustive: (),
        }));
    }
}

impl FakeChannel<(), ()> {
    pub fn default<Req, Resp>() -> FakeChannel<io::Result<Request<Req>>, Response<Resp>> {
        FakeChannel {
            stream: VecDeque::default(),
            sink: VecDeque::default(),
            config: Config::default(),
            in_flight_requests: FnvHashSet::default(),
        }
    }
}

pub trait PollExt {
    fn is_done(&self) -> bool;
}

impl<T> PollExt for Poll<Option<T>> {
    fn is_done(&self) -> bool {
        match self {
            Poll::Ready(None) => true,
            _ => false,
        }
    }
}

pub fn cx() -> Context<'static> {
    Context::from_waker(&noop_waker_ref())
}
