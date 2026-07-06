// acron — a reliable on-device cron for Android.
//
// Android's WorkManager/JobScheduler fire late or not at all under Doze/battery
// optimization. Run this as a root daemon and it fires on the wall clock like
// real cron. Crontab lines are standard 5-field cron, plus two extensions:
//   @reboot   <cmd>        run once when the daemon starts
//   @<N>s     <cmd>        run every N seconds (sub-minute; for fast testing)
// Everything after the schedule is the shell command (run via `sh -c`).

use std::io::Write;
use std::process::Command;

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let res = match args.get(1).map(String::as_str) {
        Some("run") => cmd_run(&args[2..]),
        Some("test") => cmd_test(&args[2..]),
        Some("check") => cmd_check(&args[2..]),
        _ => {
            eprintln!(
                "acron — on-device cron (root)\n\
                 \n\
                 run   <crontab> [--log F]     run the scheduler daemon\n\
                 test  <crontab> [--n K]       show next K fire times per entry\n\
                 check <crontab> [--at 'YYYY-MM-DD HH:MM']  what fires at that minute"
            );
            Err("no command".into())
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

type R<T> = Result<T, Box<dyn std::error::Error>>;

fn opt<'a>(a: &'a [String], k: &str) -> Option<&'a str> {
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(String::as_str)
}

// --- schedule model ------------------------------------------------------

enum Schedule {
    // Each field holds the set of allowed values. `dom_star`/`dow_star` record
    // whether the field was unrestricted, to implement cron's day-of-month /
    // day-of-week OR quirk.
    Cron {
        min: Vec<u32>,
        hour: Vec<u32>,
        dom: Vec<u32>,
        mon: Vec<u32>,
        dow: Vec<u32>,
        dom_star: bool,
        dow_star: bool,
    },
    Every(i64),
    Reboot,
}

struct Entry {
    sched: Schedule,
    cmd: String,
}

// Parse one cron field ("*", "*/5", "1-4", "1,3,5", "1-9/2") over [lo, hi].
fn parse_field(f: &str, lo: u32, hi: u32) -> R<(Vec<u32>, bool)> {
    let is_star = f == "*" || f.starts_with("*/");
    let mut out = Vec::new();
    for part in f.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>()?),
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((x, y)) = range.split_once('-') {
            (x.parse()?, y.parse()?)
        } else {
            let v: u32 = range.parse()?;
            (v, v)
        };
        if a < lo || b > hi || a > b || step == 0 {
            return Err(format!("bad field '{f}'").into());
        }
        let mut v = a;
        while v <= b {
            out.push(v);
            v += step;
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok((out, is_star))
}

fn parse_line(line: &str) -> R<Option<Entry>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    if let Some(cmd) = line.strip_prefix("@reboot") {
        return Ok(Some(Entry { sched: Schedule::Reboot, cmd: cmd.trim().to_string() }));
    }
    // @Ns extension: "@30s echo hi"
    if let Some(rest) = line.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let spec = it.next().unwrap_or("");
        if let Some(num) = spec.strip_suffix('s') {
            let secs: i64 = num.parse()?;
            if secs <= 0 {
                return Err("@Ns needs N > 0".into());
            }
            let cmd = it.next().unwrap_or("").trim().to_string();
            return Ok(Some(Entry { sched: Schedule::Every(secs), cmd }));
        }
        return Err(format!("unknown @ spec '{spec}'").into());
    }
    // 5 fields then the command.
    let mut parts = line.splitn(6, char::is_whitespace).filter(|s| !s.is_empty());
    let f: Vec<&str> = (&mut parts).take(5).collect();
    if f.len() < 5 {
        return Err(format!("need 5 time fields: '{line}'").into());
    }
    let cmd = parts.next().unwrap_or("").trim().to_string();
    let (min, _) = parse_field(f[0], 0, 59)?;
    let (hour, _) = parse_field(f[1], 0, 23)?;
    let (dom, dom_star) = parse_field(f[2], 1, 31)?;
    let (mon, _) = parse_field(f[3], 1, 12)?;
    let (mut dow, dow_star) = parse_field(f[4], 0, 7)?;
    // Normalize Sunday: 7 -> 0.
    if dow.contains(&7) {
        dow.retain(|&d| d != 7);
        if !dow.contains(&0) {
            dow.push(0);
        }
        dow.sort_unstable();
    }
    Ok(Some(Entry {
        sched: Schedule::Cron { min, hour, dom, mon, dow, dom_star, dow_star },
        cmd,
    }))
}

fn load(path: &str) -> R<Vec<Entry>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        match parse_line(line) {
            Ok(Some(e)) => out.push(e),
            Ok(None) => {}
            Err(e) => return Err(format!("line {}: {e}", i + 1).into()),
        }
    }
    Ok(out)
}

// Does a Cron schedule fire at this local minute?
fn cron_matches(s: &Schedule, t: &DateTime<Local>) -> bool {
    let Schedule::Cron { min, hour, dom, mon, dow, dom_star, dow_star } = s else {
        return false;
    };
    if !min.contains(&t.minute()) || !hour.contains(&t.hour()) || !mon.contains(&t.month()) {
        return false;
    }
    let d_ok = dom.contains(&t.day());
    let w_ok = dow.contains(&t.weekday().num_days_from_sunday());
    // Vixie cron quirk: when BOTH dom and dow are restricted, match if EITHER
    // does; otherwise the restricted one (or both-star) decides.
    match (*dom_star, *dow_star) {
        (false, false) => d_ok || w_ok,
        (false, true) => d_ok,
        (true, false) => w_ok,
        (true, true) => true,
    }
}

// --- commands ------------------------------------------------------------

fn spawn(cmd: &str) {
    if cmd.is_empty() {
        return;
    }
    // Detach: we don't wait, so a slow job never delays the next tick.
    let _ = Command::new("sh").arg("-c").arg(cmd).spawn();
}

fn stamp() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn cmd_run(args: &[String]) -> R<()> {
    let path = args.first().ok_or("run needs a crontab path")?;
    let entries = load(path)?;
    let log = opt(args, "--log");

    let logline = |msg: &str| {
        let line = format!("{} {msg}", stamp());
        println!("{line}");
        let _ = std::io::stdout().flush();
        if let Some(f) = log {
            if let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
                let _ = writeln!(fh, "{line}");
            }
        }
    };

    logline(&format!("acron start: {} entries from {path}", entries.len()));

    // @reboot fires once, now.
    for e in &entries {
        if matches!(e.sched, Schedule::Reboot) {
            logline(&format!("@reboot -> {}", e.cmd));
            spawn(&e.cmd);
        }
    }

    // Per-entry last-run epoch for @Ns entries.
    let mut last_every = vec![0i64; entries.len()];
    let mut last_min = -1i64;

    loop {
        let now = Local::now();
        let epoch = now.timestamp();

        // Minute-granularity cron: evaluate once when the minute rolls over.
        let this_min = epoch.div_euclid(60);
        if this_min != last_min {
            last_min = this_min;
            for e in &entries {
                if cron_matches(&e.sched, &now) {
                    logline(&format!("cron -> {}", e.cmd));
                    spawn(&e.cmd);
                }
            }
        }

        // Sub-minute @Ns entries.
        for (i, e) in entries.iter().enumerate() {
            if let Schedule::Every(secs) = e.sched {
                if epoch - last_every[i] >= secs {
                    last_every[i] = epoch;
                    logline(&format!("@{secs}s -> {}", e.cmd));
                    spawn(&e.cmd);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}

fn cmd_test(args: &[String]) -> R<()> {
    let path = args.first().ok_or("test needs a crontab path")?;
    let n: usize = opt(args, "--n").and_then(|s| s.parse().ok()).unwrap_or(3);
    let entries = load(path)?;
    let start = Local::now();

    for e in &entries {
        match &e.sched {
            Schedule::Reboot => println!("@reboot                {}", e.cmd),
            Schedule::Every(s) => println!("@{s}s (every {s}s)        {}", e.cmd),
            Schedule::Cron { .. } => {
                let mut fires = Vec::new();
                // Scan minute by minute up to ~400 days.
                let mut t = (start + Duration::minutes(1))
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap();
                for _ in 0..(400 * 24 * 60) {
                    if cron_matches(&e.sched, &t) {
                        fires.push(t.format("%Y-%m-%d %H:%M").to_string());
                        if fires.len() >= n {
                            break;
                        }
                    }
                    t += Duration::minutes(1);
                }
                println!("next {}: {}  ->  {}", n, fires.join(" | "), e.cmd);
            }
        }
    }
    Ok(())
}

fn cmd_check(args: &[String]) -> R<()> {
    let path = args.first().ok_or("check needs a crontab path")?;
    let at = match opt(args, "--at") {
        Some(s) => {
            let naive = chrono::NaiveDateTime::parse_from_str(&format!("{s}:00"), "%Y-%m-%d %H:%M:%S")
                .map_err(|e| format!("bad --at: {e}"))?;
            naive.and_local_timezone(Local).single().ok_or("ambiguous local time")?
        }
        None => Local::now(),
    };
    println!("evaluating at {}", at.format("%Y-%m-%d %H:%M (%a)"));
    for e in &entries_of(path)? {
        let fires = match &e.sched {
            Schedule::Cron { .. } => cron_matches(&e.sched, &at),
            _ => false,
        };
        if fires {
            println!("FIRE  {}", e.cmd);
        }
    }
    Ok(())
}

fn entries_of(path: &str) -> R<Vec<Entry>> {
    load(path)
}
