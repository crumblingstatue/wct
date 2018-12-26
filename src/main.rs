use rand::{rngs::ThreadRng, thread_rng, Rng};
use serde_derive::{Deserialize, Serialize};
use std::env;
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::io::{prelude::*, SeekFrom};
use std::process::Command;

enum Op {
    SetVictim(String),
    Good,
    Bad,
    Again,
    List,
    Reset,
    Times(usize),
    Save(String),
    Apply(String),
    SetRun(String),
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Op, Box<Error>> {
    args.next();
    match args.next() {
        Some(arg) => match arg.as_str() {
            "set-victim" => Ok(Op::SetVictim(args.next().ok_or("Set wot?")?)),
            "good" => (Ok(Op::Good)),
            "bad" => (Ok(Op::Bad)),
            "again" => (Ok(Op::Again)),
            "list" => (Ok(Op::List)),
            "reset" => (Ok(Op::Reset)),
            "times" => (Ok(Op::Times(args.next().ok_or("times wot?")?.parse()?))),
            "save" => (Ok(Op::Save(args.next().ok_or("save to where?")?))),
            "apply" => (Ok(Op::Apply(args.next().ok_or("apply wot?")?.parse()?))),
            "set-run" => Ok(Op::SetRun(args.next().ok_or("set run to wot?")?)),
            _ => Err(format!("Unknown command: {}", arg).into()),
        },
        None => Err("You must do something".into()),
    }
}

#[derive(Serialize, Deserialize)]
struct State {
    victim: String,
    #[serde(skip, default = "thread_rng")]
    rng: ThreadRng,
    changes: Vec<Change>,
    times: usize,
    run_command: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            victim: Default::default(),
            rng: thread_rng(),
            changes: Default::default(),
            times: 1,
            run_command: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Change {
    offset: u64,
    old: u8,
    new: u8,
}

impl State {
    pub fn set_victim(&mut self, victim: String) {
        self.victim = victim;
    }
    pub fn from_path(path: &str) -> Result<Self, Box<Error>> {
        let f = File::open(path)?;
        Ok(bincode::deserialize_from(f)?)
    }
    pub fn save_to_path(&self, path: &str) -> Result<(), Box<Error>> {
        let f = File::create(path)?;
        bincode::serialize_into(f, self)?;
        Ok(())
    }
    pub fn good(&mut self) -> Result<(), Box<Error>> {
        self.check_valid()?;
        self.corrupt_random(self.times)
    }
    pub fn bad(&mut self) -> Result<(), Box<Error>> {
        self.check_valid()?;
        self.revert(self.times)?;
        self.corrupt_random(self.times)
    }
    fn revert(&mut self, n: usize) -> Result<(), Box<Error>> {
        self.check_valid()?;
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.victim)?;
        for _ in 0..n {
            let last_change = self.changes.pop().ok_or("No change to revert")?;
            f.seek(SeekFrom::Start(last_change.offset))?;
            f.write_all(&[last_change.old])?;
            eprintln!(
                "[r] {:x}: {} <- {}",
                last_change.offset, last_change.old, last_change.new
            );
        }
        Ok(())
    }
    fn corrupt_random(&mut self, times: usize) -> Result<(), Box<Error>> {
        let rng = &mut self.rng;
        corrupt_from_source(
            &self.run_command,
            &self.victim,
            &mut self.changes,
            move |flen| (rng.gen_range(0, flen), rng.gen()),
            times,
        )
    }
    fn run(&self) {
        run(&self.run_command, &self.victim)
    }
    fn again(&self) {
        self.run();
    }
    fn list(&self) {
        for ch in &self.changes {
            println!("{:x}: {} -> {}", ch.offset, ch.old, ch.new);
        }
    }
    fn reset(&mut self) -> Result<(), Box<Error>> {
        self.revert(self.changes.len())
    }
    fn set_times(&mut self, n: usize) {
        self.times = n;
    }
    fn save(&self, path: &str) -> Result<(), Box<Error>> {
        let f = File::create(path)?;
        bincode::serialize_into(f, &self.changes)?;
        Ok(())
    }
    fn apply(&mut self, path: &str) -> Result<(), Box<Error>> {
        let f = File::open(path)?;
        let changes: Vec<Change> = bincode::deserialize_from(f)?;
        let len = changes.len();
        let mut iter = changes.into_iter();
        corrupt_from_source(
            &self.run_command,
            &self.victim,
            &mut self.changes,
            |_| {
                let chg = iter.next().unwrap();
                (chg.offset, chg.new)
            },
            len,
        )
    }
    fn set_run(&mut self, path: String) {
        self.run_command = path;
    }
    fn check_valid(&self) -> Result<(), &'static str> {
        if self.victim.is_empty() {
            Err("Victim is not set. Use `set-victim <path>`.")
        } else if self.run_command.is_empty() {
            Err("Run command is not set. Use `set-run <path>`.")
        } else {
            Ok(())
        }
    }
}

fn corrupt_from_source(
    command: &str,
    victim: &str,
    changes: &mut Vec<Change>,
    mut fun: impl FnMut(u64) -> (u64, u8),
    times: usize,
) -> Result<(), Box<Error>> {
    let mut f = OpenOptions::new().read(true).write(true).open(victim)?;
    let len = f.metadata()?.len();
    for _ in 0..times {
        let (offset, new) = fun(len);
        f.seek(SeekFrom::Start(offset))?;
        let mut old = [0u8];
        f.read_exact(&mut old)?;
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(&[new])?;
        eprintln!("[c] {:x}: {} -> {}", offset, old[0], new);
        changes.push(Change {
            offset,
            old: old[0],
            new,
        });
    }
    run(command, victim);
    Ok(())
}

fn run(command: &str, victim: &str) {
    Command::new(command).arg(victim).status().unwrap();
}

const DAT_PATH: &str = "wct.dat";

fn main() -> Result<(), Box<Error>> {
    let args = env::args();
    let mut state = State::from_path(DAT_PATH).unwrap_or_default();
    match parse_args(args)? {
        Op::SetVictim(victim) => state.set_victim(victim),
        Op::Good => state.good()?,
        Op::Bad => state.bad()?,
        Op::Again => state.again(),
        Op::List => state.list(),
        Op::Reset => state.reset()?,
        Op::Times(n) => state.set_times(n),
        Op::Save(path) => state.save(&path)?,
        Op::Apply(path) => state.apply(&path)?,
        Op::SetRun(path) => state.set_run(path),
    }
    state.save_to_path(DAT_PATH)?;
    Ok(())
}
