// textplots/plotters (used only for the CLI's console speed chart) pull in
// fontconfig, which does not cross-compile for Android - stub the module
// out there instead (it's dev/debug console output, never used by the
// FFI/mobile measurement path).
#[cfg(not(target_os = "android"))]
pub mod graph_service;
#[cfg(target_os = "android")]
pub mod graph_service {
    use crate::client::client::Measurement;

    pub struct GraphService;

    impl GraphService {
        pub fn print_graph(_state_refs: &Vec<Measurement>) {}
    }
}
pub mod json;
pub mod printer;