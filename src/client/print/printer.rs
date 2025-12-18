use prettytable::format::{FormatBuilder, LinePosition, LineSeparator, TableFormat};
use prettytable::{row, Table};

const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

const PHASE_COL_WIDTH: usize = 20;
const RESULT_COL_WIDTH: usize = 30;
const TABLE_WIDTH: usize = PHASE_COL_WIDTH + RESULT_COL_WIDTH + 6; // +6 for borders and padding

/// Creates a table format for result rows.
fn build_row_format(is_last: bool) -> TableFormat {
    let mut builder = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .padding(1, 1);

    if is_last {
        builder = builder.separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┴', '└', '┘'),
        );
    }

    builder.build()
}

/// Prints a single row to the table with the given phase and result.
fn print_row(phase: &str, result: &str, is_last: bool) {
    let mut table = Table::new();
    table.set_format(build_row_format(is_last));
    table.add_row(row![
        format!("{:<PHASE_COL_WIDTH$}", phase),
        format!("{:<RESULT_COL_WIDTH$}", result)
    ]);
    print!("{}", table);
}

pub fn print_test_result(phase: &str, status: &str, speed: Option<(f64, f64, f64)>, is_last: bool) {
    let result = match speed {
        Some((_, gbps, mbps)) => format!("{:.2} Gbit/s ({:.2} Mbit/s)", gbps, mbps),
        None => status.to_string(),
    };
    print_row(phase, &result, is_last);
}

pub fn print_raw_results(ping_ms: f64, download_gbps: f64, upload_gbps: f64) {
    println!(
        "ping/{:.2}/download/{:.2}/upload/{:.2}",
        ping_ms, download_gbps, upload_gbps
    );
}

pub fn print_test_header() {
    let title = "Nettest Broadband Test";
    let padding = (TABLE_WIDTH - title.len()) / 2;
    println!(
        "\n{}{}{}{}\n",
        " ".repeat(padding),
        GREEN,
        title,
        RESET,
    );

    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Top],
            LineSeparator::new('─', '┬', '┌', '┐'),
        )
        .separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    table.add_row(row![
        format!("{:<PHASE_COL_WIDTH$}", "Test Phase"),
        format!("{:<RESULT_COL_WIDTH$}", "Result")
    ]);
    print!("{}", table);
}

pub fn print_result(phase: &str, status: &str, speed: Option<usize>, is_last: bool) {
    let result = match speed {
        Some(mbps) => format!("{} - {:.2} ", status, mbps),
        None => status.to_string(),
    };
    print_row(phase, &result, is_last);
}

pub fn print_float_result(phase: &str, status: &str, speed: Option<f64>, is_last: bool) {
    let result = match speed {
        Some(mbps) => format!("{:.2} {}", mbps, status),
        None => status.to_string(),
    };
    print_row(phase, &result, is_last);
}
