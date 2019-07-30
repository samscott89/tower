use futures::prelude::*;
use tower::Service;
use tower::ServiceExt;


struct SlowService(usize);

impl SlowService {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn with_count(num: usize) -> Self {
        Self(num)
    }
}

impl Service<()> for SlowService {
    type Response = ();
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
    type Future = Box<Future<Item=Self::Response, Error=Self::Error> + Send>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        let curr = self.0;
        tracing::trace!({ %curr} ,"polling");
        if curr > 0 {
            self.0 -= 1;
            Ok(Async::NotReady)
        } else {
            Ok(Async::Ready(()))
        }
    }

    fn call(&mut self, _req: ()) -> Self::Future {
        Box::new(futures::future::ok(()))
    }
}

pub struct DelayService<S> {
    timer: tokio::timer::Delay,
    inner: S
}

impl<S> DelayService<S> {
    pub fn new(inner: S, time: std::time::Duration) -> Self {
        let when = std::time::Instant::now() + time;
        Self {
            timer: tokio::timer::Delay::new(when),
            inner,
        }
    }
}

impl<Request, S> Service<Request> for DelayService<S>
where
    Request: Send + 'static,
    S: Service<Request> + Send + 'static,
    S::Response: Send,
    S::Error: Send + From<tokio::timer::Error>,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Box<Future<Item=Self::Response, Error=Self::Error> + Send>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        futures::try_ready!(self.timer.poll());
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if self.timer.is_elapsed() {
            Box::new(self.inner.call(req))
        } else {
            panic!("Call poll_ready first");
        }
    }
}

pub struct ReadyingService<S>(S);

impl<S> ReadyingService<S> {
    pub fn new(inner: S) -> Self {
        Self(inner)
    }
}

impl<Request, S> Service<Request> for ReadyingService<S>
where
    Request: Send + 'static,
    S: Service<Request> + Send + 'static,
    S::Response: Send,
    S::Error: Send,
    S::Future: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Box<Future<Item=Self::Response, Error=Self::Error> + Send>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        loop {
            match self.0.poll_ready() {
                Ok(Async::Ready(_)) => {
                    tracing::trace!("ReadyingService=Ready");
                    break
                },
                Ok(Async::NotReady) => {
                    tracing::trace!("ReadyingService=NotReady");
                    continue;
                },
                Err(e) => {
                    return Box::new(futures::future::err(e));
                }
            }
        }
        let fut = self.0.call(req);
        Box::new(fut)
    }
}


fn main() {
    // let mut rt = tokio::runtime::current_thread::Runtime::new().unwrap();
    let subscriber = tracing_fmt::FmtSubscriber::builder()
        .with_filter(tracing_fmt::filter::EnvFilter::from(
            "trace",
        )).finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let svc = SlowService::with_count(2);

    let fut = futures::future::lazy(|| {
        // let svc = DelayService::new(svc, std::time::Duration::from_millis(500));
        let svc = tower_spawn_ready::SpawnReady::new(svc);
        // let svc = ReadyingService::new(svc);
        let buff = tower_buffer::Buffer::new(svc, 2);

        for _ in 0..6 {
            let mut buff_copy = buff.clone();
            if let Ok(Async::Ready(_)) = buff_copy.poll_ready() {
                tokio::spawn(buff_copy.call(()).map_err(drop));
                // std::thread::sleep_ms(100);
                // tracing::trace!({ ?res }, "polling");
            }
        }
        // tokio::spawn(buff.clone().call(()).map_err(drop));
        // tokio::spawn(buff.clone().call(()).map_err(drop));
        // tokio::spawn(buff.clone().call(()).map_err(drop));
        // buff.clone().call(()).map_err(drop)
        Ok(())
    });

    // let res: Result<(), ()> = tokio::runtime::current_thread::block_on_all(fut);

    // res.unwrap()
    tokio::run(fut);
}