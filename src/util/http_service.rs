use std::future::Future;
use std::task::{Context, Poll};

use http::{Request, Response};
use http_body::Body;
use tower_service::Service;

/// An HTTP client (like [`hyper::Client`]).
///
/// This is just an alias for [`tower_service::Service`] introduced to reduce the number of type
/// parameters.
pub trait HttpService<B>: private::Sealed<B> {
    /// Body of the responses given by the service.
    type ResponseBody: Body;
    type Error;
    type Future: Future<Output = Result<Response<Self::ResponseBody>, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    fn call(&mut self, request: Request<B>) -> Self::Future;

    fn into_service(self) -> IntoService<Self>
    where
        Self: Sized;
}

#[derive(Clone)]
pub struct IntoService<S>(S);

impl<S, ReqB, ResB> HttpService<ReqB> for S
where
    S: Service<Request<ReqB>, Response = Response<ResB>> + ?Sized,
    ResB: Body,
{
    type ResponseBody = ResB;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::poll_ready(self, cx)
    }

    fn call(&mut self, request: Request<ReqB>) -> S::Future {
        Service::call(self, request)
    }

    fn into_service(self) -> IntoService<Self>
    where
        Self: Sized,
    {
        IntoService(self)
    }
}

impl<S, ReqB, ResB> private::Sealed<ReqB> for S
where
    S: Service<Request<ReqB>, Response = Response<ResB>> + ?Sized,
    ResB: Body,
{
}

impl<S, B> Service<Request<B>> for IntoService<S>
where
    S: HttpService<B>,
{
    type Response = Response<S::ResponseBody>;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        HttpService::poll_ready(&mut self.0, cx)
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        HttpService::call(&mut self.0, request)
    }
}

mod private {
    pub trait Sealed<B> {}
}
