use futures::future;
use std::task::{Context, Poll};
use std::marker::PhantomData;
use hyper::service::Service;
use hyper::{http, Body, Request, Response, Server, StatusCode};
use sp_lfs_cache::Cache;

mod traits;
mod helpers;
#[cfg(feature = "user-data")]
pub mod user_data;

pub use traits::Resolver;
pub use helpers::b64decode;

fn not_found() -> Response<Body> {
	Response::builder()
		.status(StatusCode::NOT_FOUND)
		.body(Body::from("404 - Not found"))
		.expect("Building this simple response doesn't fail. qed")
}

struct LfsServer<C, R, L> {
	cache: C,
	resolver: R,
	_marker: PhantomData<L>,
}

impl<C, R, LfsId> LfsServer<C, R, LfsId> {
	fn new(cache: C, resolver: R) -> Self {
		Self { cache, resolver, _marker: Default::default() }
	}
}

impl<C, R, LfsId> LfsServer<C, R, LfsId>
where
	C: Cache<LfsId>,
	R: Resolver<LfsId>,
	LfsId: sp_lfs_core::LfsId,
{
	fn read_data(&self, key: LfsId) -> Option<Vec<u8>> {
		self.cache.get(&key).ok()
	}
}

impl<C, R, LfsId> Service<Request<Body>> for LfsServer<C, R, LfsId>
where
	C: Cache<LfsId>,
	R: Resolver<LfsId>,
	LfsId: sp_lfs_core::LfsId,
{
    type Response = Response<Body>;
    type Error = http::Error;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
		if let Some(it) = self.resolver.resolve(req.uri().clone()) {
			if let Some(data) = it.filter_map(|key| self.read_data(key)).next() {
				return future::ok(Response::new(data.into()))
			}
		}
        future::ok(not_found())
    }
}
 
struct MakeSvc<C, R, L>(C, R, PhantomData<L>);
impl<C, R, L> MakeSvc<C, R, L> {
	fn new(cache: C, resolver: R) -> Self {
		Self(cache,resolver, Default::default())
	}

}

impl<C, R, L, T> Service<T> for MakeSvc<C, R, L>
where
	C: Cache<L> + Clone + Send,
	R: Resolver<L> + Clone + Send,
	L: sp_lfs_core::LfsId + Send,
{
    type Response = LfsServer<C, R, L>;
    type Error = std::io::Error;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Ok(()).into()
    } 

    fn call(&mut self, _: T) -> Self::Future {
        future::ok(LfsServer::new(self.0.clone(), self.1.clone()))
    }
}



pub async fn start_server<C, R, LfsId>(cache: C, resolver: R) -> ()
where
	C: Cache<LfsId> + Clone + 'static + Send,
	R: Resolver<LfsId> + 'static + Send,
	LfsId: sp_lfs_core::LfsId + 'static,
{
	// This is our socket address...
	let addr = ([127, 0, 0, 1], 8080).into();
	let service = MakeSvc::new(cache, resolver);

	let server = Server::bind(&addr).serve(service);
	if let Err(e) = server.await {
        println!("server error: {}", e);
    }
}
