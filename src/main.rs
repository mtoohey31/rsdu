use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, stdin, stdout, Write},
    iter::FromIterator,
    sync::{mpsc, Arc, Mutex},
    thread,
};
use termion::event::{Event, Key};
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use tui::Terminal;

#[derive(Debug)]
enum PathInfo {
    File(u64),
    Folder(u64, BTreeMap<std::ffi::OsString, PathInfo>, u64),
}

impl PathInfo {
    fn size(&self) -> u64 {
        match *self {
            PathInfo::Folder(s, _, _) => s,
            PathInfo::File(s) => s,
        }
    }

    fn join(&mut self, vec: &Vec<std::ffi::OsString>) -> Result<&mut PathInfo, io::Error> {
        let mut curr_res = self;
        for comp in vec {
            match curr_res {
                PathInfo::Folder(_, ref mut c, ..) => {
                    if c.contains_key(comp) {
                        curr_res = c.get_mut(comp).unwrap();
                    } else {
                        return Err(io::Error::new(io::ErrorKind::Other, ""));
                    }
                }
                PathInfo::File(..) => return Err(io::Error::new(io::ErrorKind::Other, "")),
            };
        }
        match curr_res {
            PathInfo::Folder(..) => Ok(curr_res),
            PathInfo::File(..) => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }

    fn contents(&self) -> Result<&BTreeMap<std::ffi::OsString, PathInfo>, io::Error> {
        match self {
            PathInfo::Folder(_, c, ..) => Ok(c),
            PathInfo::File(..) => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }

    fn sorted(&self) -> Result<Vec<(&std::ffi::OsString, &PathInfo)>, io::Error> {
        match self {
            PathInfo::Folder(_, c, _) => {
                let mut contents_vec = Vec::from_iter(c.iter());
                contents_vec.sort_by(|(_, a), (_, b)| a.size().cmp(&b.size()).reverse());
                Ok(contents_vec)
            }
            _ => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }
}

fn join_path_to_vec(path: &std::path::Path, vec: Vec<std::ffi::OsString>) -> std::path::PathBuf {
    let mut tmp_path = path.to_path_buf();
    for comp in vec {
        tmp_path = tmp_path.join(comp);
    }
    tmp_path
}

// TODO: erase window on quit
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = stdout().into_raw_mode().unwrap();
    write!(stdout, "{}", termion::clear::All).unwrap();
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let starting_dir = match get_starting_dir() {
        Ok(dir) => Arc::new(Mutex::new(dir)),
        Err(e) => panic!("{}", e),
    };

    let mut state = ListState::default();
    state.select(Some(0));

    let (tx, rx) = mpsc::channel();

    let contents = Arc::new(Mutex::new(PathInfo::Folder(0, BTreeMap::new(), 0)));
    let contents_clone = Arc::clone(&contents);
    let dir: Vec<std::ffi::OsString> = vec![];
    let current_dir = Arc::new(Mutex::new(dir));
    let starting_dir_clone = Arc::clone(&starting_dir);
    thread::spawn(move || {
        *contents_clone.lock().unwrap() = get_wrapped_contents(&starting_dir_clone.lock().unwrap());
        tx.send(1).unwrap();
    });

    loop {
        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                    .split(f.size());
                let block = Paragraph::new("scanning...")
                    .block(Block::default().title(" rsdu ").borders(Borders::ALL));
                f.render_widget(block, chunks[0]);
            })
            .unwrap();
        match rx.recv().unwrap() {
            _ => break,
        }
    }

    let contents_clone = Arc::clone(&contents);
    let current_dir_clone = Arc::clone(&current_dir);
    let starting_dir_clone = Arc::clone(&starting_dir);

    terminal
        .draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                .split(f.size());
            let mut items: Vec<ListItem> = vec![];
            let mut contents_access = contents_clone.lock().unwrap();
            let joined_contents = contents_access
                .join(&*current_dir_clone.lock().unwrap())
                .unwrap();
            let display_dir = join_path_to_vec(
                &*starting_dir_clone.lock().unwrap(),
                (current_dir_clone.lock().unwrap()).clone(),
            )
            .canonicalize()
            .unwrap();
            let display_dir_string = String::from(display_dir.to_string_lossy());
            let block = Paragraph::new(display_dir_string)
                .block(Block::default().title(" rsdu ").borders(Borders::ALL));
            f.render_widget(block, chunks[0]);

            for (path, info) in joined_contents.sorted().unwrap() {
                items.push(ListItem::new(Spans::from(Span::raw(
                    String::from(pad_and_prettify_bytes(&info.size()))
                        + &size_bar(&info.size(), &joined_contents.size())
                        + &path.as_os_str().to_string_lossy()
                        + match info {
                            PathInfo::Folder(..) => "/",
                            PathInfo::File(..) => "",
                        },
                ))));
            }
            let paths = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(paths, chunks[1], &mut state);
        })
        .unwrap();

    let stdin = stdin();
    let _dir = Arc::clone(&current_dir);
    for event in stdin.events() {
        match event.unwrap() {
            Event::Key(key) => {
                match key {
                    // TODO: implement ctrl + u and ctrl + d for quicker movement, e.g.: Key::Ctrl('h')
                    // TODO: implement deletion with confirmation
                    // TODO; implement trashing with the give `trash` command found on the shell's path
                    // TODO; implement selection and application of deletion and trashing commands
                    // to all selected files
                    Key::Char('q') => break,
                    Key::Char('j') | Key::Down => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        if dir_len != 0 {
                            let new_state = (state.selected().unwrap() as isize + 1)
                                .rem_euclid(dir_len as isize)
                                as usize;
                            state.select(Some(new_state));
                        }
                    }
                    Key::Char('k') | Key::Up => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        if dir_len != 0 {
                            let new_state = (state.selected().unwrap() as isize - 1)
                                .rem_euclid(dir_len as isize)
                                as usize;
                            state.select(Some(new_state));
                        }
                    }
                    Key::Char('l') | Key::Right => {
                        let mut drawn_dir_access = current_dir_clone.lock().unwrap();
                        let mut contents_access = (&contents_clone).lock().unwrap();
                        let joined = contents_access.join(&*drawn_dir_access).unwrap();
                        let sorted = joined.sorted().unwrap();
                        let (target_os_string, info) =
                            sorted.iter().nth(state.selected().unwrap()).unwrap();
                        match info {
                            PathInfo::Folder(..) => {
                                (*drawn_dir_access)
                                    .push(std::ffi::OsString::from(target_os_string));
                                state.select(match joined {
                                    PathInfo::Folder(.., mut l) => Some(l as usize),
                                    PathInfo::File(..) => panic!(),
                                });
                            }
                            PathInfo::File(..) => {}
                        }
                    }
                    Key::Char('h') | Key::Left => {
                        let mut drawn_dir_access = current_dir_clone.lock().unwrap();
                        let mut contents_access = (&contents_clone).lock().unwrap();
                        let mut joined = contents_access.join(&*drawn_dir_access).unwrap();
                        match joined {
                            PathInfo::Folder(.., ref mut s) => {
                                *s = state.selected().unwrap() as u64;
                                // TODO: make this work
                                // println!("{}", s);
                                // println!("{}", state.selected().unwrap());
                            }
                            PathInfo::File(..) => panic!(),
                        }
                        (*drawn_dir_access).pop();
                        joined = contents_access.join(&*drawn_dir_access).unwrap();
                        state.select(match joined {
                            PathInfo::Folder(.., s) => Some(*s as usize),
                            PathInfo::File(..) => Some(panic!()),
                        });
                    }
                    Key::Char('r') => {
                        let drawn_dir_clone = current_dir_clone.lock().unwrap().clone();
                        let mut contents_access = (&contents_clone).lock().unwrap();
                        let joined = contents_access.join(&drawn_dir_clone).unwrap();
                        *joined = get_wrapped_contents(&mut join_path_to_vec(
                            &starting_dir_clone.lock().unwrap(),
                            drawn_dir_clone,
                        ));
                    }
                    Key::Char('g') => state.select(Some(0)),
                    Key::Char('G') => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        state.select(Some(dir_len - 1));
                    }
                    _ => (),
                }
            }
            _ => (),
        };
        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
                    .split(f.size());
                let mut items: Vec<ListItem> = vec![];
                let mut contents_access = contents_clone.lock().unwrap();
                let joined_contents = contents_access
                    .join(&*current_dir_clone.lock().unwrap())
                    .unwrap();
                let display_dir = join_path_to_vec(
                    &*starting_dir_clone.lock().unwrap(),
                    (current_dir_clone.lock().unwrap()).clone(),
                )
                .canonicalize()
                .unwrap();
                let display_dir_string = String::from(display_dir.to_string_lossy());
                let block = Paragraph::new(display_dir_string)
                    .block(Block::default().title(" rsdu ").borders(Borders::ALL));
                f.render_widget(block, chunks[0]);

                for (path, info) in joined_contents.sorted().unwrap() {
                    items.push(ListItem::new(Spans::from(Span::raw(
                        String::from(pad_and_prettify_bytes(&info.size()))
                            + &size_bar(&info.size(), &joined_contents.size())
                            + &path.as_os_str().to_string_lossy()
                            + match info {
                                PathInfo::Folder(..) => "/",
                                PathInfo::File(..) => "",
                            },
                    ))));
                }
                let paths = List::new(items)
                    .block(Block::default().borders(Borders::ALL))
                    .highlight_style(
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_stateful_widget(paths, chunks[1], &mut state);
            })
            .unwrap();
    }
    write!(
        io::stdout().into_raw_mode().unwrap(),
        "{}",
        termion::cursor::Show
    )
    .unwrap();
    Ok(())
}

fn get_starting_dir() -> Result<std::path::PathBuf, io::Error> {
    let current_dir = match env::current_dir() {
        Ok(dir) => dir,
        Err(e) => panic!("{}", e),
    };
    let args = env::args().collect::<Vec<String>>();
    if args.len() == 1 {
        Ok(current_dir)
    } else if args.len() == 2 {
        if &args[1][0..0] != "/" {
            Ok(std::fs::canonicalize(current_dir.join(&std::path::Path::new(&args[1]))).unwrap())
        } else {
            Ok(std::fs::canonicalize(std::path::PathBuf::from(&args[1])).unwrap())
        }
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, ""))
    }
}

fn get_wrapped_contents(dir: &std::path::Path) -> PathInfo {
    let contents = get_contents(dir).unwrap();
    PathInfo::Folder(sum_contents(&contents), contents, 0)
}

fn get_contents(
    dir: &std::path::Path,
) -> Result<BTreeMap<std::ffi::OsString, PathInfo>, io::Error> {
    let mut tree_map = BTreeMap::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return Err(e),
    };

    // TODO: multithread this and investigate ways of improving performance

    for entry in entries {
        let safe_entry = match entry {
            Ok(entry) => entry,
            Err(e) => return Err(e),
        };
        let metadata = match safe_entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) => return Err(e),
        };
        if safe_entry.path().is_dir() {
            match get_contents(&safe_entry.path()) {
                Ok(contents) => {
                    let _ = tree_map.insert(
                        std::ffi::OsString::from(
                            safe_entry.path().components().last().unwrap().as_os_str(),
                        ),
                        PathInfo::Folder(sum_contents(&contents) + metadata.len(), contents, 0),
                    );
                }
                Err(e) => return Err(e),
            }
        } else {
            tree_map.insert(
                std::ffi::OsString::from(
                    safe_entry.path().components().last().unwrap().as_os_str(),
                ),
                PathInfo::File(metadata.len()),
            );
        }
    }
    Ok(tree_map)
}

fn sum_contents(contents: &BTreeMap<std::ffi::OsString, PathInfo>) -> u64 {
    contents.values().fold(0, |acc, x| acc + x.size())
}

fn prettify_bytes(bytes: &u64) -> String {
    // Adapted from https://github.com/banyan/rust-pretty-bytes
    if bytes < &1024 {
        return bytes.to_string();
    }
    let float_bytes = *bytes as f64;
    let units = ["", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let exp = (float_bytes.ln() / 1024_f64.ln()).floor() as i32;
    format!(
        "{:.1}{}",
        float_bytes / 1024_f64.powi(exp),
        units[exp as usize]
    )
}

fn pad_and_prettify_bytes(bytes: &u64) -> String {
    let pretty_bytes = prettify_bytes(bytes);
    " ".repeat(8 - pretty_bytes.len()) + &pretty_bytes
}

fn size_bar(child_bytes: &u64, parent_bytes: &u64) -> String {
    let bar_components = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let fraction = *child_bytes as f64 / *parent_bytes as f64;
    let floored_frac = (fraction * 8_f64).floor();
    // TODO: Double check this math
    let mut bar = "█".repeat(floored_frac as usize)
        + &bar_components[((fraction - (floored_frac / 8_f64)) * 32_f64).round() as usize]
            .to_string();
    bar += &" ".repeat((7 - floored_frac as usize).min(8));
    " [".to_string() + &bar + "] "
}
