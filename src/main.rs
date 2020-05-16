use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use bytesize::ByteSize;
use clap::arg_enum;
use structopt::StructOpt;
use tabwriter::TabWriter;
use walkdir::WalkDir;

struct DirTree {
    name: OsString,
    size: u64,
    children: BTreeMap<OsString, DirTree>,
}

impl DirTree {
    fn new(root: OsString) -> DirTree {
        DirTree {
            name: root,
            size: 0,
            children: BTreeMap::new(),
        }
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

arg_enum! {
    #[derive(Copy, Clone, Debug)]
    enum SortBy {
        Name,
        Size,
    }
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(default_value = ".")]
    root: PathBuf,

    /// Number of file extensions taking up the most space to show.
    #[structopt(short = "e", long = "top-exts")]
    top_exts: Option<usize>,

    #[structopt(
        short = "s",
        long = "sort",
        possible_values = &SortBy::variants(),
        case_insensitive = true,
        default_value = "size"
    )]
    sort_by: SortBy,

    #[structopt(short = "r", long = "reverse")]
    reverse: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let opts = Opts::from_args();

    let mut dir_tree = DirTree::new(opts.root.clone().into());
    let mut ext_sizes: HashMap<OsString, u64> = HashMap::new();

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
) -> Result<(), Box<dyn Error>> {
    let mut ext_sizes = ext_sizes;
    ext_sizes.sort_by_key(|&(_, size)| Reverse(size));
    ext_sizes.truncate(limit);

    if !ascending {
        ext_sizes.reverse();
    }

    let mut tw = TabWriter::new(vec![]);
    for (ext, size) in ext_sizes.iter() {
        let size_str = format!(
            "{: >10}",
            ByteSize::b(*size).to_string().replace(" B", "  B")
        );
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
) -> Result<(), Box<dyn Error>> {
    let mut tw = TabWriter::new(vec![]);
    let mut directories = dir_tree.children.values().collect::<Vec<_>>();
    match sort_by {
        SortBy::Name if reverse => directories.sort_by_key(|d| Reverse(d.name.clone())),
        SortBy::Name => directories.sort_by_key(|d| d.name.clone()),
        SortBy::Size if reverse => directories.sort_by_key(|d| Reverse(d.size)),
        SortBy::Size => directories.sort_by_key(|d| d.size),
    }

    for dir in &directories {
        let size_str = format!(
            "{: >10}",
            ByteSize::b(dir.size).to_string().replace(" B", "  B")
        );
        writeln!(
            &mut tw,
            "{}\t{}",
            format!("{: >8}", size_str),
            root.join(dir.name.clone()).to_string_lossy()
        )?;
    }

    let size_str = format!(
        "{: >10}",
        ByteSize::b(dir_tree.size).to_string().replace(" B", "  B")
    );

    if directories.len() > 0 {
        writeln!(&mut tw, "----------")?;
    }

    writeln!(
        &mut tw,
        "{:>8}\t{}",
        format!("{: >8}", size_str),
        dir_tree.name.to_string_lossy()
    )?;
    tw.flush()?;
    io::stdout().write_all(&tw.into_inner()?)?;

    Ok(())
}
