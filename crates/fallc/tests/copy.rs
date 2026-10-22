//! Copies of owning containers.
//!
//! A container that owns a buffer must not be copied member-wise: the two
//! values would free one buffer. These tests copy, assign and move each owning
//! container and read both sides afterwards, so a shallow copy shows up as a
//! wrong answer now or a crash at the double free.

use std::path::PathBuf;

use fallc::cli::Options;
use fallc::pipeline::run;

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory =
            std::env::temp_dir().join(format!("fallc-copy-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a temporary directory can be created");
        Scratch { directory }
    }

    fn run(&self, label: &str, source: &str) -> String {
        let main = self.directory.join("main.ft");
        std::fs::write(&main, source).expect("the source can be written");
        let executable = self.directory.join(label);
        let options = Options::parse(&[
            main.display().to_string(),
            "-o".to_owned(),
            executable.display().to_string(),
        ])
        .expect("the arguments are well formed");
        assert_eq!(
            run(&options),
            std::process::ExitCode::SUCCESS,
            "`{label}` did not compile"
        );
        let output = std::process::Command::new(&executable)
            .output()
            .expect("the program can be run");
        assert!(
            output.status.success(),
            "`{label}` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.directory.as_path());
    }
}

#[test]
fn tree_and_hash_containers_copy_deeply() {
    // Copies, assignments and moves of every owning container, then both sides
    // are read: a shallow copy shows as a wrong count or a double free.
    let source = r#"import std;
import C;

fn main() int except {
    map TMap<String, int> = TMap<String, int>();
    map.insert(String("a"), 1);
    map.insert(String("b"), 2);
    map_copy TMap<String, int> = map;
    map_copy.insert(String("c"), 3);
    C.printf("map %d %d %d\n", map.length() as int, map_copy.length() as int, *map_copy.find(String("c")));

    map_assigned TMap<String, int> = TMap<String, int>();
    map_assigned = map;
    C.printf("assigned %d %d\n", map_assigned.length() as int, *map_assigned.find(String("b")));

    map_moved TMap<String, int> = std.move(map_assigned);
    C.printf("moved %d\n", map_moved.length() as int);

    set TSet<int> = TSet<int>();
    set.insert(4);
    set.insert(5);
    set_copy TSet<int> = set;
    C.printf("set %d %d\n", set.length() as int, set_copy.length() as int);

    multi TMMap<String, int> = TMMap<String, int>();
    multi.insert(String("k"), 1);
    multi.insert(String("k"), 2);
    multi_copy TMMap<String, int> = multi;
    C.printf("multi %d %d\n", multi.length() as int, multi_copy.length() as int);

    hmap HMap<String, int> = HMap<String, int>();
    hmap.insert(String("x"), 10);
    hmap_copy HMap<String, int> = hmap;
    hmap_copy.insert(String("y"), 20);
    C.printf("hash %d %d %d\n", hmap.length() as int, hmap_copy.length() as int, *hmap_copy.find(String("y")));

    hset HSet<int> = HSet<int>();
    hset.insert(7);
    hset_copy HSet<int> = hset;
    C.printf("hset %d %d\n", hset.length() as int, hset_copy.length() as int);

    hm HMMap<String, int> = HMMap<String, int>();
    hm.insert(String("z"), 1);
    hm.insert(String("z"), 2);
    hm_copy HMMap<String, int> = hm;
    C.printf("hmmap %d %d\n", hm.length() as int, hm_copy.length() as int);

    q Queue<int> = Queue<int>();
    q.push(1);
    q.push(2);
    q_copy Queue<int> = q;
    C.printf("queue %d %d %d\n", q.pop(), q_copy.pop(), q_copy.pop());

    st Stack<int> = Stack<int>();
    st.push(9);
    st_copy Stack<int> = st;
    C.printf("stack %d %d\n", st.pop(), st_copy.pop());
    return 0;
}
"#;
    let scratch = Scratch::new("all");
    assert_eq!(
        scratch.run("all", source),
        "map 2 3 3\nassigned 2 2\nmoved 2\nset 2 2\nmulti 2 2\nhash 1 2 20\nhset 1 1\nhmmap 2 2\nqueue 1 1 2\nstack 9 9\n"
    );
}
