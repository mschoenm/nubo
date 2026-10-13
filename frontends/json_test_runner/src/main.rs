//! Runs the GameboyCPUTests JSON suite: thousands of single-instruction tests
//! that check the CPU registers, the memory and every memory access after
//! each instruction.

use nubo_core::{BusActivityEntry, BusActivityKind, Emulator, Registers};
use serde::Deserialize;
use std::any::Any;
use std::env;
use std::fs;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

/// One byte of memory, written in the JSON files as `[address, value]`.
#[derive(Debug, Deserialize)]
struct MemoryByte {
    address: u16,
    value: u8,
}

/// The CPU registers and the memory the test sets up or expects.
#[derive(Debug, Deserialize)]
struct CpuState {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    f: u8,
    h: u8,
    l: u8,
    pc: u16,
    sp: u16,
    ram: Vec<MemoryByte>,
}

/// Whether the CPU read or wrote memory in an M-cycle.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum AccessKind {
    Read,
    Write,
}

/// The memory access the CPU makes in one M-cycle (a machine cycle, four
/// clock ticks), written in the JSON files as `[address, value, kind]`.
#[derive(Debug, Deserialize)]
struct MemoryAccess {
    address: u16,
    value: u8,
    kind: AccessKind,
}

/// One test: the state before one instruction, the state after it, and the
/// memory access of each M-cycle in between (`None` for M-cycles without
/// one).
#[derive(Debug, Deserialize)]
struct CpuTest {
    name: String,
    initial: CpuState,
    #[serde(rename = "final")]
    expected: CpuState,
    cycles: Vec<Option<MemoryAccess>>,
}

/// The `.json` files in `directory`, sorted by name (the opcode in
/// hexadecimal). Fails if `directory` isn't a readable directory.
fn json_test_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Reads the tests in one JSON file.
fn read_json_tests(path: &Path) -> Result<Vec<CpuTest>, String> {
    let data = fs::read_to_string(path).map_err(|error| format!("failed to read it: {error}"))?;
    serde_json::from_str(&data).map_err(|error| format!("failed to parse it: {error}"))
}

/// Whether the emulator's memory access in one M-cycle is the expected one.
fn access_matches(expected: Option<&MemoryAccess>, actual: Option<&BusActivityEntry>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => {
            let actual_kind = match actual.kind {
                BusActivityKind::Read => AccessKind::Read,
                BusActivityKind::Write => AccessKind::Write,
            };
            expected.address == actual.address
                && expected.value == actual.value
                && expected.kind == actual_kind
        }
        (None, None) => true,
        _ => false,
    }
}

/// Describes how the memory accesses differ from the expected ones, or
/// returns `None` if they match M-cycle for M-cycle.
fn bus_activity_difference(
    expected: &[Option<MemoryAccess>],
    actual: &[Option<BusActivityEntry>],
) -> Option<String> {
    if expected.len() != actual.len() {
        return Some(format!(
            "took {} M-cycles instead of {}: {actual:?}",
            actual.len(),
            expected.len()
        ));
    }
    let cycle = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| !access_matches(expected.as_ref(), actual.as_ref()))?;
    Some(format!(
        "M-cycle {cycle} accessed {:?} instead of {:?}",
        actual[cycle], expected[cycle]
    ))
}

/// Runs one test: sets up the registers and memory, runs one instruction and
/// compares the registers, the memory and the memory accesses with the
/// expected ones. The emulator runs in its CPU test mode
/// (`Emulator::for_cpu_tests`), where the whole 64 KiB address space is
/// plain RAM.
fn run_cpu_test(test: &CpuTest) -> Result<(), String> {
    let mut emulator = Emulator::for_cpu_tests();
    let initial = &test.initial;

    for byte in &initial.ram {
        emulator.write_test_memory(byte.address, byte.value);
    }

    // The CPU fetches each opcode during the instruction before it, so the
    // test starts with the opcode before `pc` already fetched.
    let opcode = emulator.read_test_memory(initial.pc.wrapping_sub(1));

    emulator.set_registers(Registers {
        a: initial.a,
        b: initial.b,
        c: initial.c,
        d: initial.d,
        e: initial.e,
        f: initial.f,
        h: initial.h,
        l: initial.l,
        opcode,
        program_counter: initial.pc,
        stack_pointer: initial.sp,
    });

    emulator.step();

    let registers = emulator.registers();
    let expected = &test.expected;
    let registers_match = registers.a == expected.a
        && registers.b == expected.b
        && registers.c == expected.c
        && registers.d == expected.d
        && registers.e == expected.e
        && registers.f == expected.f
        && registers.h == expected.h
        && registers.l == expected.l
        && registers.program_counter == expected.pc
        && registers.stack_pointer == expected.sp;
    if !registers_match {
        return Err(format!(
            "test {}: the registers are {registers:?}, expected {expected:?}",
            test.name
        ));
    }

    if let Some(byte) = expected
        .ram
        .iter()
        .find(|byte| emulator.read_test_memory(byte.address) != byte.value)
    {
        return Err(format!(
            "test {}: memory at {:#06X} holds {:#04X}, expected {:#04X}",
            test.name,
            byte.address,
            emulator.read_test_memory(byte.address),
            byte.value
        ));
    }

    if let Some(difference) = bus_activity_difference(&test.cycles, emulator.bus_activity()) {
        return Err(format!("test {}: {difference}", test.name));
    }
    Ok(())
}

/// Runs every test in one file and returns how many there were, or why the
/// file failed: it couldn't be read, a test failed, or the emulator panicked.
fn run_test_file(path: &Path) -> Result<usize, String> {
    let tests = read_json_tests(path)?;
    if tests.is_empty() {
        return Err("it holds no tests".to_string());
    }
    let result = panic::catch_unwind(AssertUnwindSafe(|| tests.iter().try_for_each(run_cpu_test)));
    match result {
        Ok(Ok(())) => Ok(tests.len()),
        Ok(Err(failure)) => Err(failure),
        Err(payload) => Err(format!("panicked: {}", panic_message(payload.as_ref()))),
    }
}

/// The message of a caught panic, if it has one.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap_or_else(|| "unknown panic".to_string())
}

/// Runs the files on all CPU cores and returns each file's result, in the
/// order of `files`. A file whose result went missing, because its worker
/// thread stopped, counts as failed.
fn run_test_files(files: &[PathBuf]) -> Vec<Result<usize, String>> {
    let workers = thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let next = AtomicUsize::new(0);

    let finished: Vec<(usize, Result<usize, String>)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut finished = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(index) else {
                            return finished;
                        };
                        finished.push((index, run_test_file(file)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    });

    let mut results: Vec<Result<usize, String>> = files
        .iter()
        .map(|_| Err("no result: the thread running it stopped".to_string()))
        .collect();
    for (index, result) in finished {
        results[index] = result;
    }
    results
}

/// Finds the tests (downloading them unless a directory is given), runs
/// them and reports the result. Fails when there are no test files, or when
/// any file fails.
fn main() -> ExitCode {
    let tests_dir = match env::args().nth(1).map(PathBuf::from) {
        Some(path) => path,
        None => match download::ensure_cpu_tests() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::FAILURE;
            }
        },
    };

    let files = match json_test_files(&tests_dir) {
        Ok(files) if files.is_empty() => {
            eprintln!("error: {} holds no .json test files", tests_dir.display());
            return ExitCode::FAILURE;
        }
        Ok(files) => files,
        Err(error) => {
            eprintln!(
                "error: could not read the directory {}: {error}",
                tests_dir.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let results = run_test_files(&files);

    let mut passed_tests = 0;
    let mut failed_files = 0;
    for (file, result) in files.iter().zip(&results) {
        match result {
            Ok(count) => passed_tests += count,
            Err(error) => {
                failed_files += 1;
                println!("{}: {}", file.display(), error);
            }
        }
    }

    println!(
        "Ran {} test files in {:.1}s: {} tests passed, {} files failed",
        files.len(),
        start.elapsed().as_secs_f64(),
        passed_tests,
        failed_files
    );

    if failed_files == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

mod download;
