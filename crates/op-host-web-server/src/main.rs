//! Headless OpenPencil web / MCP server binary.
//!
//! Links only `op-host-services` (the extracted headless daemon) — no
//! winit / glutin / muda / accesskit adapters / skia-GL — so the
//! container / server image stays GUI-free. The `--serve-web` / `--mcp` /
//! `--mcp-http` dispatch is shared with the desktop binary via
//! [`op_host_services::cli_modes::run_cli_mode`], but here that IS the
//! whole program: there is no GUI fallback, so an unknown / missing mode
//! is a usage error rather than "open the editor window".

use std::process::exit;

/// The stack the daemon's work runs on.
///
/// Unix gives a main thread 8 MiB and Windows gives it 1 MiB, and this program's
/// startup needs a little over one megabyte: **measured on macOS** by capping the
/// main stack, the same binary dies with `thread 'main' … has overflowed its
/// stack` at `ulimit -s 1024` and comes up normally at `ulimit -s 2048`.
///
/// That is the whole of issue #217's Windows failures — three tests spawn this
/// binary and read a handshake line from its stdout, and on Windows the child was
/// killed by its own stack before it could print one, so each test reported an
/// empty handshake as a JSON parse error and the daemon never got to say anything
/// about itself.
///
/// So the work runs on a thread with an explicit stack rather than on whichever
/// one the platform hands `main`. The recursion that eats the megabyte is **not**
/// identified — #217 stays open for that, and this is a portability fix rather
/// than a repair of it — but a program whose startup depends on the platform's
/// default stack is a program that only works on the platforms it was tried on,
/// and 8 MiB is what every platform it has run on so far gave it.
const MAIN_STACK_BYTES: usize = 8 * 1024 * 1024;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        eprintln!(
            "op-host-web-server: missing mode. Usage:\n  \
             --serve-web <port> [doc] [--host <addr>]   headless web-canvas daemon\n  \
             --mcp <path>                               JSON-RPC stdio MCP server\n  \
             --mcp-http <port> <path>                   Streamable-HTTP MCP server"
        );
        exit(2);
    };
    // The mode's own arguments are collected before the thread starts, so the
    // worker owns everything it needs and `main` only waits for a status.
    let rest: Vec<String> = args.collect();

    let daemon = std::thread::Builder::new()
        .name("op-host-web-server".to_string())
        .stack_size(MAIN_STACK_BYTES)
        .spawn(move || {
            match op_host_services::cli_modes::run_cli_mode(
                "op-host-web-server",
                &first,
                rest.into_iter(),
            ) {
                Some(code) => code,
                None => {
                    eprintln!(
                        "op-host-web-server: unknown mode {first:?} \
                         (expected --serve-web / --mcp / --mcp-http)"
                    );
                    2
                }
            }
        })
        .expect("spawn the daemon thread");

    let code = match daemon.join() {
        Ok(code) => code,
        // A panic inside the daemon has already printed its own message and
        // backtrace; what the caller needs from here is a non-zero status.
        Err(_) => {
            eprintln!("op-host-web-server: the daemon thread panicked");
            101
        }
    };
    if code != 0 {
        exit(code);
    }
}
