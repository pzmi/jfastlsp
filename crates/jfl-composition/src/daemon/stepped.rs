//! The host stepped one action at a time (IDX-12's obligation 9): the daemon conformance driver
//! and the state-machine test take each model action as one call, on a current-thread runtime
//! this facade owns, so the test crates name no Tokio. Each call returns once its step is done;
//! nothing runs between calls.

use std::io;

use jfl_protocol::rpc::RequestId;

use super::host::{ConnId, Host, HostConfig, HostError, Received, Tick};
use super::service::Unwired;

/// A [`Host`] and the runtime its steps run on.
#[derive(Debug)]
pub struct SteppedHost {
    rt: tokio::runtime::Runtime,
    host: Host,
    service: Unwired,
}

impl SteppedHost {
    /// A started host for `config` (the in-process daemon of a spawn).
    pub fn new(config: HostConfig) -> io::Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        Ok(Self {
            rt,
            host: Host::new(config),
            service: Unwired,
        })
    }

    /// The host, to observe.
    #[must_use]
    pub const fn host(&self) -> &Host {
        &self.host
    }

    /// [`Host::lock`].
    pub fn lock(&mut self) -> Result<(), HostError> {
        self.host.lock()
    }

    /// [`Host::unlink_stale`].
    pub fn unlink_stale(&mut self) -> Result<(), HostError> {
        self.host.unlink_stale()
    }

    /// [`Host::listen`].
    pub fn listen(&mut self) -> Result<(), HostError> {
        let _runtime = self.rt.enter();
        self.host.listen()
    }

    /// [`Host::accept`]: the queued connection whose handshake id is `hello`.
    pub fn accept(&mut self, hello: &RequestId) -> Result<ConnId, HostError> {
        self.rt.block_on(self.host.accept(hello))
    }

    /// [`Host::receive`]: one frame of `conn`.
    pub fn receive(&mut self, conn: ConnId) -> Result<Received, HostError> {
        self.rt.block_on(self.host.receive(conn))
    }

    /// [`Host::answer`] through [`Unwired`].
    pub fn answer(&mut self, conn: ConnId) -> Result<bool, HostError> {
        self.rt.block_on(self.host.answer(conn, &mut self.service))
    }

    /// [`Host::reap`].
    pub fn reap(&mut self, conn: ConnId) -> Result<(), HostError> {
        self.host.reap(conn)
    }

    /// [`Host::idle_exit`].
    pub fn idle_exit(&mut self) -> Result<(), HostError> {
        self.host.idle_exit()
    }

    /// [`Host::stop`] (SIGTERM's first step).
    pub fn stop(&mut self) -> Result<(), HostError> {
        self.host.stop()
    }

    /// [`Host::stop_alone`] (the model's fault `spawnLockWaitExpires`).
    pub fn stop_alone(&mut self) -> Result<(), HostError> {
        self.host.stop_alone()
    }

    /// [`Host::unlink`].
    pub fn unlink(&mut self) -> Result<(), HostError> {
        self.host.unlink()
    }

    /// [`Host::release`].
    pub fn release(&mut self) -> Result<(), HostError> {
        self.host.release()
    }

    /// [`Host::crash`].
    pub fn crash(&mut self) {
        self.host.crash();
    }

    /// [`Host::tick`].
    pub fn tick(&self) -> Result<Tick, HostError> {
        self.host.tick()
    }
}
