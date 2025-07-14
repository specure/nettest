use prettytable::format::{FormatBuilder, LinePosition, LineSeparator};
use prettytable::{row, Table};

const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

pub fn print_test_result(phase: &str, status: &str, speed: Option<(f64, f64, f64)>) {
    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    let result = if let Some((_, gbps, mbps)) = speed {
        format!("{} - {:.2} Gbit/s ({:.2} Mbit/s)", status, gbps, mbps)
    } else {
        status.to_string()
    };

    table.add_row(row![format!("{:<30}", phase), format!("{:<40}", result)]);
    println!("{}", table);
}

pub fn print_test_header() {
    // Print centered green title
    let title = "Nettest Broadband Test";
    let table_width = 74; // 30 + 40 + 4 (padding and borders)
    let padding = (table_width - title.len()) / 2;
    println!(
        "\n{}{}{}{}{}",
        " ".repeat(padding),
        GREEN,
        title,
        RESET,
        " ".repeat(padding)
    );
    println!();

    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Top, LinePosition::Bottom],
            LineSeparator::new('─', '┼', '┌', '┐'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    table.add_row(row![
        format!("{:<30}", "Test Phase"),
        format!("{:<40}", "Result")
    ]);
    println!("{}", table);
}



pub fn print_result(phase: &str, status: &str, speed: Option<usize>) {
    let mut table = Table::new();
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[LinePosition::Bottom],
            LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    let result = if let Some(mbps) = speed {
        format!("{} - {:.2} ", status, mbps)
    } else {
        status.to_string()
    };

    table.add_row(row![format!("{:<30}", phase), format!("{:<40}", result)]);
    println!("{}", table);
}

