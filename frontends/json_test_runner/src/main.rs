use retroboy_core::emulator::{Emulator, Registers, BusActivityEntry, BusActivityType};
use serde::Deserialize;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;

#[derive(Debug, Deserialize)]
struct RamEntry(u16, u8);

#[derive(Debug, Deserialize)]
struct JsonCpuState {
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
    ram: Vec<RamEntry>,
}

#[derive(Debug, Deserialize)]
struct CycleEntry(u16, u8, String);

#[derive(Debug, Deserialize)]
struct JsonCpuTest {
    name: String,
    initial: JsonCpuState,
    r#final: JsonCpuState,
    cycles: Vec<Option<CycleEntry>>,
}

const JSON_CPU_TESTS_PATH: &str = "../../../GameboyCPUTests/v2";

fn list_files_in_path(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            }
        }
    } else {
        println!("The specified path is not a directory.");
    }

    Ok(files)
}

fn sort_files_by_hex_value(files: Vec<PathBuf>) -> Vec<PathBuf> {
    // Each JSON test's file name is a hexadecimal number from 0x00 to 0xFF.
    // This function just reads the file name and sorts by the hexadecimal value.
    let mut files_with_hex: Vec<(u8, PathBuf)> = files
        .into_iter()
        .filter_map(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|file_name| u8::from_str_radix(file_name, 16).ok())
                .map(|hex_value| (hex_value, path))
        })
        .collect();

    files_with_hex.sort_by_key(|(hex_value, _)| *hex_value);

    files_with_hex.into_iter().map(|(_, path)| path).collect()
}

fn read_json_tests(path: &Path) -> Result<Vec<JsonCpuTest>, io::Error> {
    let data = fs::read_to_string(path)?;
    let json_objects: Vec<JsonCpuTest> = serde_json::from_str(&data)?;
    Ok(json_objects)
}

fn is_json_file(file: &PathBuf) -> bool {
    file.to_string_lossy().ends_with(".json")
}

fn collect_json_test_files() -> Result<Vec<PathBuf>, io::Error> {
    let path = Path::new(JSON_CPU_TESTS_PATH);
    let files = list_files_in_path(path)?;
    let json_files: Vec<PathBuf> = files.into_iter().filter(is_json_file).collect();
    let sorted_files = sort_files_by_hex_value(json_files);
    Ok(sorted_files)
}

fn bus_activity_matches(
    expected_cycles: &Vec<Option<CycleEntry>>,
    actual_cycles: &Vec<Option<BusActivityEntry>>
) -> bool {
    expected_cycles.iter().zip(actual_cycles.iter()).all(|(maybe_expected_cycle, maybe_actual_cycle)| {
        match (maybe_expected_cycle, maybe_actual_cycle) {
            (Some(expected_cycle), Some(actual_cycle)) => {
                let expected_address = expected_cycle.0;
                let expected_value = expected_cycle.1;
                let expected_activity = &expected_cycle.2;

                let actual_address = actual_cycle.address();
                let actual_value = actual_cycle.value();
                let actual_activity = &actual_cycle.activity_type();

                expected_address == actual_address &&
                expected_value == actual_value &&
                match (expected_activity.as_str(), actual_activity) {
                    ("read", BusActivityType::Read) | ("write", BusActivityType::Write) => true,
                    _ => false,
                }
            }
            (None, None) => true,
            _ => false,
        }
    })
}

fn run_cpu_test(test: &JsonCpuTest) {
    let mut emulator = Emulator::new(true);

    for entry in &test.initial.ram {
        emulator.set_processor_test_ram(entry.0, entry.1);
    }

    let opcode = emulator.processor_test_ram_byte(test.initial.pc - 1);
    
    let mut initial_register_state = Registers::default();

    initial_register_state
        .set_a(test.initial.a)
        .set_b(test.initial.b)
        .set_c(test.initial.c)
        .set_d(test.initial.d)
        .set_e(test.initial.e)
        .set_f(test.initial.f)
        .set_h(test.initial.h)
        .set_l(test.initial.l)
        .set_opcode(opcode)
        .set_program_counter(test.initial.pc)
        .set_stack_pointer(test.initial.sp);

    emulator.set_register_state(&initial_register_state);

    emulator.step();

    let next_register_state = emulator.register_state();

    let test_failed = next_register_state.a() != test.r#final.a ||
        next_register_state.b() != test.r#final.b ||
        next_register_state.c() != test.r#final.c ||
        next_register_state.d() != test.r#final.d ||
        next_register_state.e() != test.r#final.e ||
        next_register_state.f() != test.r#final.f ||
        next_register_state.h() != test.r#final.h ||
        next_register_state.l() != test.r#final.l ||
        next_register_state.program_counter() != test.r#final.pc ||
        next_register_state.stack_pointer() != test.r#final.sp ||
        !bus_activity_matches(&test.cycles, emulator.opcode_bus_activity());

    if test_failed {
        panic!("Test {} failed: CPU state: {:?}, Expected: {:?}", test.name, next_register_state, test.r#final);
    }
}

fn main() -> io::Result<()> {
    let json_test_files = collect_json_test_files()?;

    for json_test_file in json_test_files {
        match read_json_tests(&json_test_file) {
            Ok(tests) => {
                for test in tests {
                    run_cpu_test(&test);
                }
            }
            Err(e) => {
                println!("Error reading JSON tests: {}", e);
            }
        }
        println!("Successfully ran opcode tests in file: {}", json_test_file.display());
    }

    Ok(())
}
