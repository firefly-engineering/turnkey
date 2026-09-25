//! Protocol code generated from pinned upstream protos: buck2's test-runner
//! protocol and the Remote Execution API
//! (see nix/packages/test-runner-protocol.nix).
//!
//! TURNKEY_TEST_RUNNER_PROTOCOL names the generated code's directory at
//! compile time: Buck2 sets it from the `[turnkey] test_runner_protocol`
//! config, Cargo builds inherit it from the dev shell or the Nix package.

#![allow(clippy::all)]

macro_rules! generated {
    ($file:literal) => {
        include!(concat!(env!("TURNKEY_TEST_RUNNER_PROTOCOL"), "/", $file));
    };
}

pub mod buck {
    pub mod data {
        generated!("buck.data.rs");
        pub mod error {
            generated!("buck.data.error.rs");
        }
    }
    pub mod host_sharing {
        generated!("buck.host_sharing.rs");
    }
    pub mod test {
        generated!("buck.test.rs");
    }
}

pub mod build {
    pub mod bazel {
        pub mod remote {
            pub mod execution {
                pub mod v2 {
                    generated!("build.bazel.remote.execution.v2.rs");
                }
            }
        }
        pub mod semver {
            generated!("build.bazel.semver.rs");
        }
    }
}

pub mod google {
    pub mod api {
        generated!("google.api.rs");
    }
    pub mod longrunning {
        generated!("google.longrunning.rs");
    }
    pub mod rpc {
        generated!("google.rpc.rs");
    }
}
