use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

const INPUT_HEADER: &str = "type,client,tx,amount\n";
const OUTPUT_HEADER: &str = "client,available,held,total,locked";

struct InputFile {
    directory: PathBuf,
    path: PathBuf,
}

impl InputFile {
    fn new(contents: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("payments-engine-cli-{}-{id}", std::process::id()));
            match fs::create_dir(&directory) {
                Ok(()) => break directory,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("could not create test directory: {error}"),
            }
        };
        let input = Self {
            path: directory.join("transactions input.csv"),
            directory,
        };
        fs::write(&input.path, contents).unwrap();
        input
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_payments-engine"));
        command.current_dir(&self.directory);
        command
    }

    fn run(&self) -> Output {
        self.command()
            .arg(self.path.file_name().unwrap())
            .output()
            .unwrap()
    }
}

impl Drop for InputFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn assert_report(output: Output, expected_rows: &[&str]) {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with('\n'), "missing final newline: {stdout:?}");
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some(OUTPUT_HEADER));
    // Account order is intentionally unspecified.
    let mut actual: Vec<_> = lines.collect();
    let mut expected = expected_rows.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

fn assert_failure(output: Output) -> String {
    assert!(!output.status.success(), "CLI unexpectedly succeeded");
    assert!(
        output.stdout.is_empty(),
        "failure produced an account report"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.is_empty(), "failure had no diagnostic");
    assert!(!stderr.contains("panicked at"), "CLI panicked: {stderr}");
    stderr
}

#[test]
fn sample_csv_produces_expected_accounts() {
    let input = InputFile::new(include_str!("../assets/test.csv"));
    assert_report(
        input.run(),
        &[
            "1,1.5000,0.0000,1.5000,false",
            "2,2.0000,0.0000,2.0000,false",
        ],
    );
}

#[test]
fn all_supported_decimal_precisions_are_processed_exactly() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,1\n",
        "deposit,1,2,0.2\n",
        "deposit,1,3,0.03\n",
        "deposit,1,4,0.004\n",
        "deposit,1,5,0.0005\n",
        "withdrawal,1,6,0.0001\n",
    ));
    assert_report(input.run(), &["1,1.2344,0.0000,1.2344,false"]);
}

#[test]
fn csv_supports_whitespace_quotes_crlf_and_reordered_columns() {
    let input = InputFile::new(concat!(
        " tx , amount , type , client \r\n",
        "4294967295,\" 2.5000 \",\" deposit \",65535\r\n",
        "0,1,deposit,0\r\n",
        "1,0.5,withdrawal,65535\r\n",
        "4294967295,,dispute,65535\r\n",
    ));
    assert_report(
        input.run(),
        &[
            "0,1.0000,0.0000,1.0000,false",
            "65535,-0.5000,2.5000,2.0000,false",
        ],
    );
}

#[test]
fn disputes_resolutions_chargebacks_and_freezing_flow_through_the_cli() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,10\n",
        "deposit,1,2,2\n",
        "withdrawal,1,3,8.5\n",
        "dispute,1,1,\n",
        "resolve,1,1,\n",
        "dispute,1,1,\n",
        "dispute,1,1,\n",
        "chargeback,1,1,\n",
        "deposit,1,4,100\n",
        "withdrawal,1,5,0.1\n",
        "dispute,1,2,\n",
        "resolve,1,2,\n",
        "deposit,2,6,5.1234\n",
        "dispute,2,6,\n",
    ));
    assert_report(
        input.run(),
        &[
            "1,-6.5000,0.0000,-6.5000,true",
            "2,0.0000,5.1234,5.1234,false",
        ],
    );
}

#[test]
fn invalid_dispute_references_are_ignored_without_aborting_the_file() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,10\n",
        "deposit,2,2,20\n",
        "dispute,2,1,\n",
        "resolve,1,1,\n",
        "chargeback,1,1,\n",
        "dispute,1,99,\n",
        "dispute,1,1,\n",
        "resolve,2,1,\n",
        "chargeback,2,1,\n",
        "withdrawal,2,3,5\n",
    ));
    assert_report(
        input.run(),
        &[
            "1,0.0000,10.0000,10.0000,false",
            "2,15.0000,0.0000,15.0000,false",
        ],
    );
}

#[test]
fn negative_fraction_below_one_keeps_its_sign_in_output() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,1\n",
        "withdrawal,1,2,0.0001\n",
        "dispute,1,1,\n",
        "chargeback,1,1,\n",
    ));
    assert_report(input.run(), &["1,-0.0001,0.0000,-0.0001,true"]);
}

#[test]
fn total_can_exceed_the_i64_range() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,922337203685477.5807\n",
        "dispute,1,1,\n",
        "deposit,1,2,922337203685477.5807\n",
    ));
    assert_report(
        input.run(),
        &["1,922337203685477.5807,922337203685477.5807,1844674407370955.1614,false"],
    );
}

#[test]
fn empty_and_header_only_files_emit_exactly_one_header() {
    for contents in ["", INPUT_HEADER] {
        assert_report(InputFile::new(contents).run(), &[]);
    }
}

#[test]
fn invalid_amounts_report_the_original_value_and_reason() {
    for (amount, reason) in [
        ("-1", "expected decimal digits"),
        ("1.-2", "expected decimal digits"),
        ("1.+2", "expected decimal digits"),
        ("abc", "expected decimal digits"),
        ("1.2.3", "invalid fractional part"),
        (".5", "invalid whole part"),
        ("1.", "invalid fractional part"),
        ("1.12345", "at most four decimal places"),
        ("922337203685477.5808", "exceeds the supported range"),
        ("999999999999999999999999", "invalid whole part"),
    ] {
        let input = InputFile::new(&format!(
            "{INPUT_HEADER}deposit,1,1,1\ndeposit,1,2,{amount}\n"
        ));
        let stderr = assert_failure(input.run());
        assert!(
            stderr.contains(amount),
            "missing input {amount:?}: {stderr}"
        );
        assert!(
            stderr.contains(reason),
            "missing reason {reason:?}: {stderr}"
        );
    }
}

#[test]
fn invalid_csv_rows_fail_without_producing_a_report() {
    for row in [
        "deposit,1,2,",
        "withdrawal,1,2,",
        "unknown,1,2,1",
        "dispute,1,1,1",
        "deposit,65536,2,1",
        "deposit,1,4294967296,1",
        "deposit,client,2,1",
        "deposit,1,2,1,extra",
    ] {
        let input = InputFile::new(&format!("{INPUT_HEADER}deposit,1,1,1\n{row}\n"));
        assert_failure(input.run());
    }
}

#[test]
fn missing_required_csv_header_is_reported() {
    let input = InputFile::new("type,client,amount\ndeposit,1,1\n");
    let stderr = assert_failure(input.run());
    assert!(
        stderr.contains("tx"),
        "missing field not identified: {stderr}"
    );
}

#[test]
fn processing_overflow_is_reported_without_panicking_or_outputting_accounts() {
    let input = InputFile::new(concat!(
        "type,client,tx,amount\n",
        "deposit,1,1,922337203685477.5807\n",
        "deposit,1,2,0.0001\n",
    ));
    let stderr = assert_failure(input.run());
    assert!(stderr.contains("arithmetic"), "unexpected error: {stderr}");
}

#[test]
fn absolute_input_path_and_redirected_output_work() {
    let input = InputFile::new(&format!("{INPUT_HEADER}deposit,1,1,2.5\n"));
    let report_path = input.directory.join("accounts.csv");
    let mut output = input
        .command()
        .arg(&input.path)
        .stdout(Stdio::from(fs::File::create(&report_path).unwrap()))
        .output()
        .unwrap();
    assert_eq!(output.stdout, [] as [u8; 0]);
    output.stdout = fs::read(report_path).unwrap();
    assert_report(output, &["1,2.5000,0.0000,2.5000,false"]);
}

#[test]
fn cli_requires_exactly_one_argument() {
    let input = InputFile::new(INPUT_HEADER);
    assert_failure(input.command().output().unwrap());
    assert_failure(
        input
            .command()
            .arg(&input.path)
            .arg("extra")
            .output()
            .unwrap(),
    );
}

#[test]
fn missing_input_file_is_reported() {
    let input = InputFile::new(INPUT_HEADER);
    assert_failure(input.command().arg("missing.csv").output().unwrap());
}

#[cfg(target_os = "linux")]
#[test]
fn buffered_output_errors_produce_a_failure_exit_status() {
    let input = InputFile::new(&format!("{INPUT_HEADER}deposit,1,1,1\n"));
    let output = input
        .command()
        .arg(&input.path)
        .stdout(Stdio::from(
            fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        ))
        .output()
        .unwrap();
    assert_failure(output);
}
