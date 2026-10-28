//! Fixed test expectations only, never a production invocation pin provider.
use boole_gate_supervisor::{
    real_artifact::local::Profile, real_controller::InvocationPins, real_descriptor::Descriptor,
    real_reply::Context,
};
pub struct Pins {
    pub decoder: Option<[u8; 32]>,
    pub transport: Option<[u8; 32]>,
}
impl InvocationPins for Pins {
    fn decoder(&self, _descriptor: &Descriptor, _profile: Profile) -> Option<[u8; 32]> {
        self.decoder
    }
    fn transport(&self, _context: &Context) -> Option<[u8; 32]> {
        self.transport
    }
}
