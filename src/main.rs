// TODO: Catch errors when scanning and display them to the user, then continue
// TODO: Display scanning animation when refreshing too
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    iter::FromIterator,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use tui::Terminal;

#[derive(Debug)]
enum PathInfo {
    File(u64),
    Folder(u64, BTreeMap<OsString, PathInfo>, usize),
}

impl PathInfo {
    fn size(&self) -> u64 {
        match *self {
            PathInfo::Folder(s, _, _) => s,
            PathInfo::File(s) => s,
        }
    }

    fn join(&mut self, vec: &Vec<OsString>) -> Result<&mut PathInfo, io::Error> {
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

    fn contents(&self) -> Result<&BTreeMap<OsString, PathInfo>, io::Error> {
        match self {
            PathInfo::Folder(_, c, ..) => Ok(c),
            PathInfo::File(..) => Err(io::Error::new(io::ErrorKind::Other, "")),
        }
    }

    fn sorted(&self) -> Result<Vec<(&OsString, &PathInfo)>, io::Error> {
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

fn join_path_to_vec(path: &Path, vec: Vec<OsString>) -> PathBuf {
    let mut tmp_path = path.to_path_buf();
    for comp in vec {
        tmp_path = tmp_path.join(comp);
    }
    tmp_path
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout().into_raw_mode().unwrap();
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let starting_dir = match get_starting_dir() {
        Ok(dir) => Arc::new(Mutex::new(dir)),
        Err(e) => panic!("{}", e),
    };

    let state = Arc::new(Mutex::new(ListState::default()));
    state.lock().unwrap().select(Some(0));

    let (tx, rx) = std::sync::mpsc::channel();

    let contents = Arc::new(Mutex::new(PathInfo::Folder(0, BTreeMap::new(), 0)));
    let contents_clone = Arc::clone(&contents);
    let dir: Vec<OsString> = vec![];
    let current_dir = Arc::new(Mutex::new(dir));
    let starting_dir_clone = Arc::clone(&starting_dir);
    thread::spawn(move || {
        *contents_clone.lock().unwrap() = get_wrapped_contents(&starting_dir_clone.lock().unwrap());
        tx.send(0).unwrap();
    });

    let starting_dir_clone = Arc::clone(&starting_dir);
    let starting_dir_copy = starting_dir_clone.lock().unwrap().clone();
    let mut dot_pos = 0;
    let mut dot_fwd = true;
    loop {
        terminal
            .draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(
                        [
                            Constraint::Length(3),
                            Constraint::Length((f.size().height - 6) / 2),
                            Constraint::Length(3),
                        ]
                        .as_ref(),
                    )
                    .split(f.size());
                let display_dir_string = String::from(starting_dir_copy.to_string_lossy());
                let block = Paragraph::new(display_dir_string)
                    .block(Block::default().title(" rsdu ").borders(Borders::ALL));
                f.render_widget(block, chunks[0]);
                let blank1 = Block::default();
                f.render_widget(blank1, chunks[1]);
                let msg = Paragraph::new(
                    "Scanning".to_string()
                        + &" ".repeat(dot_pos)
                        + "..."
                        + &" ".repeat(6 - dot_pos),
                )
                .alignment(Alignment::Center)
                .block(Block::default());
                f.render_widget(msg, chunks[2]);
            })
            .unwrap();
        if dot_pos == 6 {
            dot_fwd = false;
        } else if dot_pos == 0 {
            dot_fwd = true;
        }
        if dot_fwd {
            dot_pos += 1;
        } else {
            dot_pos -= 1;
        }
        // TODO: Determine better way of terminating immediately without having to wait for last
        // sleep
        thread::sleep(std::time::Duration::from_millis(50));
        match rx.try_recv() {
            Ok(_) => break,
            Err(_) => {}
        }
    }

    let contents_clone = Arc::clone(&contents);
    let current_dir_clone = Arc::clone(&current_dir);
    let starting_dir_clone = Arc::clone(&starting_dir);
    let state_clone = Arc::clone(&state);

    let mut draw = move || {
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
                f.render_stateful_widget(paths, chunks[1], &mut state_clone.lock().unwrap());
            })
            .unwrap();
    };

    draw();

    let contents_clone = Arc::clone(&contents);
    let current_dir_clone = Arc::clone(&current_dir);
    let starting_dir_clone = Arc::clone(&starting_dir);
    let state_clone = Arc::clone(&state);

    let stdin = io::stdin();
    let _dir = Arc::clone(&current_dir);
    for event in stdin.events() {
        match event.unwrap() {
            termion::event::Event::Key(key) => {
                match key {
                    // TODO: implement deletion with confirmation
                    // TODO: implement trashing with the give `trash` command found on the shell's path
                    // TODO: implement selection and application of deletion and trashing commands
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
                            let new_state =
                                (((state_clone.lock().unwrap().selected().unwrap() as isize) + 1)
                                    .max(0) as usize)
                                    .min(dir_len - 1);
                            state_clone.lock().unwrap().select(Some(new_state));
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
                            let new_state =
                                (((state_clone.lock().unwrap().selected().unwrap() as isize) - 1)
                                    .max(0) as usize)
                                    .min(dir_len - 1);
                            state_clone.lock().unwrap().select(Some(new_state));
                        }
                    }
                    Key::Char('l') | Key::Right => {
                        let mut drawn_dir_access = current_dir_clone.lock().unwrap();
                        let mut contents_access = (&contents_clone).lock().unwrap();
                        let mut joined = contents_access.join(&*drawn_dir_access).unwrap();
                        match joined {
                            PathInfo::Folder(.., ref mut s) => {
                                *s = state_clone.lock().unwrap().selected().unwrap();
                            }
                            PathInfo::File(..) => panic!(),
                        }
                        let sorted = joined.sorted().unwrap();
                        let (target_os_string, info) = sorted
                            .iter()
                            .nth(state_clone.lock().unwrap().selected().unwrap())
                            .unwrap();
                        match info {
                            PathInfo::Folder(..) => {
                                (*drawn_dir_access).push(OsString::from(target_os_string));
                                joined = contents_access.join(&*drawn_dir_access).unwrap();
                                state_clone.lock().unwrap().select(match joined {
                                    PathInfo::Folder(_, _, s) => {
                                        let new_state = *s as usize;
                                        Some(new_state)
                                    }
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
                                *s = state_clone.lock().unwrap().selected().unwrap();
                            }
                            PathInfo::File(..) => panic!(),
                        }
                        (*drawn_dir_access).pop();
                        joined = contents_access.join(&*drawn_dir_access).unwrap();
                        state_clone.lock().unwrap().select(match joined {
                            PathInfo::Folder(.., s) => Some(*s as usize),
                            PathInfo::File(..) => panic!(),
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
                    Key::Char('g') => state_clone.lock().unwrap().select(Some(0)),
                    Key::Char('G') => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        state_clone.lock().unwrap().select(Some(dir_len - 1));
                    }
                    Key::Ctrl('d') | Key::Ctrl('f') => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        if dir_len != 0 {
                            let new_state = (((state_clone.lock().unwrap().selected().unwrap()
                                as isize)
                                + (termion::terminal_size().unwrap().1 as isize / 4))
                                .max(0) as usize)
                                .min(dir_len);
                            state_clone.lock().unwrap().select(Some(new_state));
                        }
                    }
                    Key::Ctrl('u') | Key::Ctrl('b') => {
                        let dir_len = (&contents_clone)
                            .lock()
                            .unwrap()
                            .join(&*current_dir_clone.lock().unwrap())
                            .unwrap()
                            .contents()
                            .unwrap()
                            .len();
                        if dir_len != 0 {
                            let new_state = (((state_clone.lock().unwrap().selected().unwrap()
                                as isize)
                                - (termion::terminal_size().unwrap().1 as isize / 4))
                                .max(0) as usize)
                                .min(dir_len);
                            state_clone.lock().unwrap().select(Some(new_state));
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        };
        draw();
    }
    Ok(())
}

fn get_starting_dir() -> Result<PathBuf, io::Error> {
    let current_dir = match env::current_dir() {
        Ok(dir) => dir,
        Err(e) => return Err(e),
    };
    let args = env::args().collect::<Vec<String>>();
    match args.len() {
        1 => Ok(current_dir),
        2 => Ok(PathBuf::from(&args[1])),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "")),
    }
}

fn get_wrapped_contents(dir: &Path) -> PathInfo {
    let threads = Arc::new(Mutex::new(1));
    let max_threads = num_cpus::get();
    let contents = get_contents(dir, threads, max_threads).unwrap();
    PathInfo::Folder(sum_contents(&contents), contents, 0)
}

fn get_contents(
    dir: &Path,
    threads: Arc<Mutex<usize>>,
    max_threads: usize,
) -> Result<BTreeMap<OsString, PathInfo>, io::Error> {
    let contents = Arc::new(Mutex::new(BTreeMap::new()));
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied => return Ok(BTreeMap::new()),
        Err(e) => panic!("{}", e),
    };

    let mut handlers = Vec::new();

    for entry in entries {
        let contents_clone = Arc::clone(&contents);
        // TODO: handle errors in threads
        let threads_depth_clone = Arc::clone(&threads);
        let task = move || {
            let safe_entry = entry.unwrap();
            let metadata = fs::symlink_metadata(safe_entry.path()).unwrap();
            if metadata.is_dir() {
                let sub_contents =
                    get_contents(&safe_entry.path(), threads_depth_clone, max_threads).unwrap();
                contents_clone.lock().unwrap().insert(
                    OsString::from(safe_entry.path().components().last().unwrap().as_os_str()),
                    PathInfo::Folder(
                        sum_contents(&sub_contents) + metadata.len(),
                        sub_contents,
                        0,
                    ),
                );
            } else {
                contents_clone.lock().unwrap().insert(
                    OsString::from(safe_entry.path().components().last().unwrap().as_os_str()),
                    PathInfo::File(metadata.len()),
                );
            };
        };
        if *threads.lock().unwrap() < max_threads {
            let threads_breadth_clone = Arc::clone(&threads);
            handlers.push(thread::spawn(move || {
                *threads_breadth_clone.lock().unwrap() += 1;
                task();
                *threads_breadth_clone.lock().unwrap() -= 1;
            }));
        } else {
            task();
        }
    }

    for handler in handlers {
        match handler.join() {
            Ok(_) => {}
            Err(_) => {}
        };
    }

    Ok(Arc::try_unwrap(contents).unwrap().into_inner().unwrap())
}

fn sum_contents(contents: &BTreeMap<OsString, PathInfo>) -> u64 {
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
    let bar_components = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let fraction = *child_bytes as f64 / *parent_bytes as f64;
    let floored_frac = (fraction * 8_f64).floor().max(0_f64);
    let mut bar = "█".repeat(floored_frac as usize)
        + &bar_components[(((fraction - (floored_frac / 8_f64)) * 64_f64).round() as usize)
            .min(8)
            .max(0)]
        .to_string();
    bar += &" ".repeat((7 - floored_frac as usize).min(8));
    " [".to_string() + &bar + "] "
}
