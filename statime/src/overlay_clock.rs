extern crate alloc;
use alloc::boxed::Box;
use fixed::traits::LossyInto;

use crate::{time::Time, time::Duration, Clock};

type ExportCallback = Box<dyn FnMut(&[u8; 32]) + Send + 'static>;

/// An overlay over other, read-only clock, frequency-locked to it.
/// In other words, a virtual clock which can be tuned in software without affecting
/// the underlying system or hardware clock.
//#[derive(Debug)]
pub struct OverlayClock<C: Clock> {
    roclock: C,
    last_sync: Time,
    shift: Duration,
    freq_scale_ppm_diff: f64,
    exporter: Option<ExportCallback>
}

impl<C: Clock> OverlayClock<C> {
    /// Creates new OverlayClock based on given clock
    pub fn new(underlying_clock: C) -> Self {
        let now = underlying_clock.now();
        Self {
            roclock: underlying_clock,
            last_sync: now,
            shift: Duration::from_fixed_nanos(0),
            freq_scale_ppm_diff: 0.0,
            exporter: None
        }
    }

    /// Converts (shifts and scales) `Time` in underlying clock's timescale to overlay clock timescale
    pub fn time_from_underlying(&self, roclock_time: Time) -> Time {
        let elapsed = roclock_time - self.last_sync;
        let corr = elapsed * self.freq_scale_ppm_diff / 1_000_000;

        roclock_time + self.shift + corr
        // equals self.last_sync + self.shift + elapsed + corr
    }

    /// Returns reference to underlying clock
    pub fn underlying(&self) -> &C {
        &self.roclock
    }

    /// Subscribes to state changes which will be exported as bytes buffer.
    /// 
    /// Only one subscribe callback can be active at time. If this functions is called multiple times, the most recent callback will be used.
    pub fn subscribe(&mut self, cb: ExportCallback) {
        self.exporter = Some(cb);
    }

    /// Disables subscription
    pub fn unsubscribe(&mut self) {
        self.exporter = None;
    }

    fn export(&mut self) {
        if let Some(export_cb) = &mut self.exporter {
            let mut buf = [0u8; 32];
            buf[0..8].copy_from_slice(b"TAIovl\x00\x01");
            let last_sync: i128 = self.last_sync.nanos().lossy_into();
            buf[8..16].copy_from_slice(&(last_sync as i64).to_ne_bytes());
            let shift: i128 = self.shift.nanos().lossy_into();
            buf[16..24].copy_from_slice(&(shift as i64).to_ne_bytes());
            buf[24..32].copy_from_slice(&(self.freq_scale_ppm_diff/1_000_000.0).to_ne_bytes());
            (export_cb)(&buf);
        }
    }
}

impl<C: Clock> Clock for OverlayClock<C> {
    type Error = C::Error;
    fn now(&self) -> Time {
        self.time_from_underlying(self.roclock.now())
    }
    fn set_frequency(&mut self, ppm: f64) -> Result<Time, Self::Error> {
        // save current shift:
        let now_roclock = self.roclock.now();
        let now_local = self.time_from_underlying(now_roclock);
        self.shift = now_local - now_roclock;
        self.last_sync = now_roclock;

        self.freq_scale_ppm_diff = ppm;
        debug_assert_eq!(self.time_from_underlying(self.last_sync), now_local);

        self.export();
        Ok(now_local)
    }
    fn step_clock(&mut self, offset: Duration) -> Result<Time, Self::Error> {
        self.last_sync = self.roclock.now();
        self.shift += offset;
        self.export();
        Ok(self.time_from_underlying(self.last_sync))
    }
    fn set_properties(&mut self, _time_properties_ds: &crate::config::TimePropertiesDS) -> Result<(), Self::Error> {
        // we can ignore the properies - they are just metadata
        Ok(())
    }
}
