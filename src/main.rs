use std::cmp::Reverse;
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ahash::AHashMap;
use argh::FromArgs;
use bytesize::ByteSize;
use tabwriter::TabWriter;
use walkdir::WalkDir;

#[derive(Debug)]
enum Error {
    InvalidSort(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSort(param) => write!(f, "invalid sort parameter: {}", param),
        }
    }
}

impl std::error::Error for Error {}

struct DirTree {
    name: OsString,
    size: u64,
    children: AHashMap<OsString, DirTree>,
}

impl DirTree {
    fn new(root: OsString) -> DirTree {
        DirTree { name: root, size: 0, children: AHashMap::new() }
    }

    fn add_dir(&mut self, path: &Path) {
        let mut parent = self;
        for dir in path {
            parent = parent
                .children
                .entry(dir.to_owned())
                .or_insert_with(|| DirTree::new(dir.to_owned()));
        }
    }

    fn add_file(&mut self, path: &Path, size: u64) {
        self.size += size;

        let mut parent = self;
        let mut dirs = path.iter().peekable();
        while let Some(dir) = dirs.next() {
            // skip the last Compoenent of the path as it's the file's name
            if dirs.peek().is_none() {
                continue;
            }

            parent = parent
                .children
                .entry(dir.to_owned())
                .or_insert_with(|| DirTree::new(dir.to_owned()));
            parent.size += size;
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum SortBy {
    Name,
    Size,
}

impl FromStr for SortBy {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "name" => Ok(Self::Name),
            "size" => Ok(Self::Size),
            _ => Err(Error::InvalidSort(s.to_owned())),
        }
    }
}

#[derive(FromArgs)]
#[argh(description = "What's Eating Space? Find out which directories take up storage.")]
struct Opts {
    /// number of file extensions taking up the most space to display
    #[argh(option, short = 'e', long = "top-exts")]
    top_exts: Option<usize>,

    /// how to sort the results (size, name) [default: size]
    #[argh(option, short = 's', long = "sort", default = "SortBy::Size")]
    sort_by: SortBy,

    /// reverse the sort order
    #[argh(switch, short = 'r', long = "reverse")]
    reverse: bool,

    #[argh(positional, default = r#"".".into()"#)]
    root: PathBuf,
}

fn main() {
    match run() {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Failed: {}", err);
        }
    };
}

fn run() -> Result<(), Box<dyn StdError>> {
    let opts: Opts = argh::from_env();

    let mut dir_tree = DirTree::new(opts.root.clone().into());
    let mut ext_sizes: AHashMap<OsString, u64> = AHashMap::new();

    for entry in WalkDir::new(&opts.root) {
        match entry {
            Ok(entry) => {
                let path = entry.path().strip_prefix(&opts.root)?;
                let meta = entry.metadata()?;

                if meta.is_dir() {
                    dir_tree.add_dir(path);
                } else {
                    let size = meta.len();
                    dir_tree.add_file(path, size);
                    if let Some(ext) = path.extension() {
                        let total = ext_sizes.entry(ext.to_owned()).or_insert(0);
                        *total += size;
                    }
                }
            }
            Err(e) => {
                eprintln!("Unable to read directory structure: {}", e);
            }
        }
    }

    if let Some(top_ext_limit) = opts.top_exts {
        println!("Top {} file types by space usage:", top_ext_limit);
        print_top_extensions(top_ext_limit, ext_sizes.into_iter().collect(), opts.reverse)?;
        println!();
    }

    print_space_usage(&opts.root, dir_tree, opts.sort_by, opts.reverse)?;

    Ok(())
}

fn print_top_extensions(
    limit: usize,
    ext_sizes: Vec<(OsString, u64)>,
    ascending: bool,
) -> Result<(), Box<dyn StdError>> {
    let mut ext_sizes = ext_sizes;
    ext_sizes.sort_by_key(|&(_, size)| Reverse(size));
    ext_sizes.truncate(limit);

    if !ascending {
        ext_sizes.reverse();
    }

    let mut tw = TabWriter::new(vec![]);
    for (ext, size) in ext_sizes.iter() {
        let size_str = format!("{: >10}", ByteSize::b(*size).to_string().replace(" B", "  B"));
        writeln!(&mut tw, "{}\t{}", size_str, ext.to_string_lossy())?;
    }
    tw.flush()?;
    io::stdout().write_all(&tw.into_inner()?)?;

    Ok(())
}

fn print_space_usage(
    root: &Path,
    dir_tree: DirTree,
    sort_by: SortBy,
    reverse: bool,
) -> Result<(), Box<dyn StdError>> {
    let mut tw = TabWriter::new(vec![]);
    let mut directories = dir_tree.children.values().collect::<Vec<_>>();
    match sort_by {
        SortBy::Name if reverse => directories.sort_by_key(|d| Reverse(d.name.clone())),
        SortBy::Name => directories.sort_by_key(|d| d.name.clone()),
        SortBy::Size if reverse => directories.sort_by_key(|d| Reverse(d.size)),
        SortBy::Size => directories.sort_by_key(|d| d.size),
    }

    for dir in &directories {
        let size_str = format!("{: >10}", ByteSize::b(dir.size).to_string().replace(" B", "  B"));
        writeln!(
            &mut tw,
            "{}\t{}",
            format!("{: >8}", size_str),
            root.join(dir.name.clone()).to_string_lossy()
        )?;
    }

    let size_str = format!("{: >10}", ByteSize::b(dir_tree.size).to_string().replace(" B", "  B"));

    if !directories.is_empty() {
        writeln!(&mut tw, "----------")?;
    }

    writeln!(&mut tw, "{:>8}\t{}", format!("{: >8}", size_str), dir_tree.name.to_string_lossy())?;
    tw.flush()?;
    io::stdout().write_all(&tw.into_inner()?)?;

    Ok(())
}
