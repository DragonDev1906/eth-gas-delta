use std::{cmp::Ordering, collections::HashMap, fs::File, path::Path, vec};

use clap::Parser;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tabled::{
    builder::Builder,
    settings::{object::Columns, Alignment, Modify, Style},
};

#[derive(Debug, Serialize, Deserialize)]
struct GasReport {
    info: Info,
}

#[derive(Debug, Serialize, Deserialize)]
struct Info {
    methods: HashMap<String, RawMethod>,
    deployments: Vec<RawDeployment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawDeployment {
    name: String,
    #[serde(rename = "gasData")]
    gas_data: Vec<isize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawMethod {
    key: String,
    #[serde(flatten)]
    method: MethodIdentifier,
    #[serde(rename = "fnSig")]
    signature: String,
    #[serde(rename = "gasData")]
    gas_data: Vec<isize>,
    #[serde(rename = "numberOfCalls")]
    number_of_calls: usize,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MethodIdentifier {
    contract: String,
    method: String,
}

impl PartialOrd for MethodIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MethodIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.contract == other.contract {
            self.method.cmp(&other.method)
        } else {
            self.contract.cmp(&other.contract)
        }
    }
}

#[derive(Debug, Clone)]
enum Entry {
    Deployment(RawDeployment),
    Method(RawMethod),
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Deployment(l0), Self::Deployment(r0)) => l0.name == r0.name,
            (Self::Method(l0), Self::Method(r0)) => l0.method == r0.method,
            _ => false,
        }
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Entry::Deployment(l), Entry::Deployment(r)) => l.name.cmp(&r.name),
            (Entry::Deployment(_), Entry::Method(_)) => Ordering::Greater,
            (Entry::Method(_), Entry::Deployment(_)) => Ordering::Less,
            (Entry::Method(l), Entry::Method(r)) => l.method.cmp(&r.method),
        }
    }
}

impl Entry {
    fn avg_gas(&self) -> isize {
        let gas_data = self.gas_data();
        let sum: isize = gas_data.iter().sum();
        sum / gas_data.len() as isize
    }

    fn has_gas_data(&self) -> bool {
        self.gas_data().len() > 0
    }

    fn gas_data(&self) -> &Vec<isize> {
        match self {
            Entry::Deployment(depl) => &depl.gas_data,
            Entry::Method(m) => &m.gas_data,
        }
    }
}

const BLOCK_LIMIT: isize = 30_000_000;
const MARGIN: f64 = 0.1;

fn main() {
    let args = Args::parse();

    // let mut reports: Vec<GasReport> = vec![];

    let mut data: HashMap<String, Vec<Option<Entry>>> = HashMap::new();
    let mut filenames: Vec<&str> = Vec::new();

    let file_count = args.files.len();
    for (index, path) in args.files.iter().enumerate() {
        let file = File::open(path).unwrap();
        let report: GasReport = serde_json::from_reader(file).unwrap();

        for depl in report.info.deployments {
            if depl.gas_data.len() == 0 {
                continue;
            }
            let key = depl.name.to_owned();
            data.entry(key).or_insert(vec![None; file_count])[index] =
                Some(Entry::Deployment(depl));
        }
        for (_, method) in report.info.methods {
            if method.gas_data.len() == 0 {
                continue;
            }
            let key = format!(
                "\x1b[90m{}.\x1b[0m{}",
                method.method.contract, method.method.method
            );
            data.entry(key).or_insert(vec![None; file_count])[index] = Some(Entry::Method(method))
        }

        filenames.push(
            Path::file_name(Path::new(path))
                .unwrap_or_default()
                .to_str()
                .unwrap(),
        );
    }

    // let data = vec![("Hello", "World"), ("123", "456"), ("ABC", "XYZ")];

    // let table = Table::new(data).with(Style::psql()).to_string();
    let mut builder = Builder::default();
    let mut header = vec!["Deployments"];
    // header.append(&mut args.files.iter().map(|s| &s[..]).collect());
    header.append(&mut filenames);

    builder.set_header(header);
    for (contract_name, entry) in data.iter().sorted() {
        let mut row: Vec<String> = vec![];
        row.push(contract_name.clone());
        let mut first_avg_gas = None;
        for (i, entry) in entry.iter().enumerate() {
            match entry {
                Some(entry) if entry.has_gas_data() => {
                    let avg: isize = entry.avg_gas();
                    row.push(match first_avg_gas {
                        Some(first_avg) => {
                            let percent =
                                100f64 * (avg as f64 - first_avg as f64) / first_avg as f64;
                            let color_code = if percent > MARGIN {
                                91 // Red
                            } else if percent < -MARGIN {
                                92 // Green
                            } else {
                                0 // White
                            };
                            format!(
                                "\x1b[{}m{:+} ({:+5.1}%)\x1b[0m",
                                color_code,
                                avg - first_avg,
                                percent
                            )
                        }
                        None => {
                            format!(
                                "{} ({:4.1}%)",
                                avg,
                                100f64 * avg as f64 / BLOCK_LIMIT as f64
                            )
                        }
                    });
                    if i == 0 {
                        first_avg_gas = Some(avg);
                    }
                }
                _ => {
                    row.push("".to_owned());
                }
            }
        }
        builder.push_record(row);
    }

    let table = builder
        .build()
        .with(Style::rounded())
        .with(Modify::new(Columns::new(1..)).with(Alignment::right()))
        .to_string();

    println!("{}", table);
}
