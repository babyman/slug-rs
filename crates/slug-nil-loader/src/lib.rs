//! Restricted-host external import loader.
//!
//! This empty seam will become the deny-all loader. It must never acquire
//! filesystem, environment, configuration, Clutch, network, or native-library
//! behavior.
