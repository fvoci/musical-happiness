use super::*;

/// The central headers are counted wherever they are, a chunk's end included, and
/// the count stops once it is over the limit.
#[test]
fn central_headers_are_counted_across_chunks() {
    let count = |bytes: &[u8], limit: u64| {
        count_central_headers(&mut &bytes[..], limit).expect("a slice reads")
    };
    assert_eq!(count(b"", 10), 0, "nothing");
    assert_eq!(count(b"PK\x01", 10), 0, "a part of one");
    assert_eq!(count(b"PK\x01\x02PK\x01\x02xPK\x01\x02", 10), 3, "three");
    for at in SCAN_CHUNK_BYTES - 3..=SCAN_CHUNK_BYTES {
        let mut bytes = vec![0; SCAN_CHUNK_BYTES * 2];
        bytes[at..at + 4].copy_from_slice(CENTRAL_HEADER);
        assert_eq!(count(&bytes, 10), 1, "one at {at}, on the chunk's edge");
    }
    let many = CENTRAL_HEADER.repeat(SCAN_CHUNK_BYTES);
    let in_a_chunk = u64::try_from(SCAN_CHUNK_BYTES / 4).expect("a small count");
    assert_eq!(count(&many, 5), in_a_chunk, "stops after the chunk");
}

/// A reader that gives a few bytes at a time, as a file may.
struct Trickle<'b>(&'b [u8], usize);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.1.min(buf.len()).min(self.0.len());
        let (head, rest) = self.0.split_at(n);
        buf.get_mut(..n).unwrap_or_default().copy_from_slice(head);
        self.0 = rest;
        Ok(n)
    }
}

/// Short reads count as whole ones: a signature split between reads is found once.
#[test]
fn central_headers_are_counted_over_short_reads() {
    let mut bytes = Vec::new();
    for gap in 0..200 {
        bytes.extend_from_slice(CENTRAL_HEADER);
        bytes.extend(std::iter::repeat_n(b'x', gap % 9));
    }
    let expected = u64::try_from(
        bytes
            .windows(4)
            .filter(|window| window == CENTRAL_HEADER)
            .count(),
    )
    .expect("a small count");
    for step in [1, 2, 3, 5, 7, 4096] {
        let counted = count_central_headers(&mut Trickle(&bytes, step), u64::MAX)
            .expect("a reader that does not fail");
        assert_eq!(counted, expected, "{step} bytes a read");
    }
}

fn block(id: &str, parent_id: &str) -> Block {
    Block {
        id: id.to_owned(),
        parent_id: parent_id.to_owned(),
        ..Block::default()
    }
}

fn ids_after_dropping(blocks: Vec<Block>) -> (Vec<String>, usize) {
    let mut archive = ContainersAndBlocks {
        containers: vec![Container {
            id: "board".to_owned(),
            ..Container::default()
        }],
        blocks,
    };
    let dropped = drop_orphans(&mut archive);
    let ids = archive.blocks.into_iter().map(|block| block.id).collect();
    (ids, dropped)
}

#[test]
fn blocks_that_nothing_leads_to_are_dropped_with_what_is_below_them() {
    let (kept, dropped) = ids_after_dropping(vec![
        block("text", "card"),
        block("card", "board"),
        block("view", ""),
        block("lost", "gone"),
        block("below-lost", "lost"),
        block("deeper", "below-lost"),
        block("ring-a", "ring-b"),
        block("ring-b", "ring-a"),
        block("own", "own"),
        block("reply", "text"),
    ]);
    assert_eq!(
        kept,
        ["text", "card", "view", "reply"],
        "what a board leads to stays, in the order it came"
    );
    assert_eq!(
        dropped, 6,
        "the lost ones, and the rings that hang off nothing"
    );
}

/// A chain as long as an archive may be: the walk is one pass, where taking the
/// orphans away level by level took one pass per level.
#[test]
fn a_long_chain_of_orphans_is_dropped_in_one_walk() {
    let length = 200_000;
    let mut blocks = vec![block("0", "missing")];
    blocks.extend((1..length).map(|index| block(&index.to_string(), &(index - 1).to_string())));
    blocks.push(block("card", "board"));

    let started = std::time::Instant::now();
    let (kept, dropped) = ids_after_dropping(blocks);

    assert_eq!(kept, ["card"]);
    assert_eq!(dropped, length);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn clean_dir_resolves_dot_segments() {
    assert_eq!(clean_dir("board1"), "board1", "a plain directory");
    assert_eq!(clean_dir("board1/../.."), "..", "traversal beyond the root");
    assert_eq!(clean_dir("a/./b"), "a/b", "a current-directory segment");
}

#[test]
fn split_entry_name_takes_the_base_name() {
    assert_eq!(
        split_entry_name("board1/../../x"),
        ("..".to_owned(), "x".to_owned()),
        "the file name is the last segment"
    );
    assert_eq!(
        split_entry_name("version.json"),
        (".".to_owned(), "version.json".to_owned()),
        "a bare name is in the current directory"
    );
}

/// A board line's names of its files are taken as an upload's name would be, with the
/// extension of the entry's name; any other is left out, and its file keeps the name of
/// its entry.
#[test]
fn the_names_of_files_are_taken_as_an_upload_names_them() {
    let longest = format!("{}.pdf", "x".repeat(MAX_UPLOAD_NAME_CHARS - 4));
    let board = serde_json::json!({"id": "b1", "fileNames": {
        "7a.pdf": "회의록.pdf",
        "7b.pdf": "renamed.exe",
        "7c.pdf": "  ",
        "7d.pdf": "nul\u{0}.pdf",
        "7e.pdf": format!("{}.pdf", "x".repeat(MAX_UPLOAD_NAME_CHARS)),
        "7f.pdf": 3,
        "7g.pdf": longest,
        "7h.jpg": "Photo.JPEG",
        "7i.pdf": "../../minutes.pdf",
        "7j.pdf": "dir/  ",
    }});
    let names = file_names_of(&board);
    let taken = |entry: &str, name: &str| (entry.to_owned(), name.to_owned());
    assert_eq!(
        names,
        FileNames::from([
            taken("7a.pdf", "회의록.pdf"),
            taken("7g.pdf", &longest),
            taken("7h.jpg", "Photo.JPEG"),
            // Whose record keeps the part after the last `/` alone (`FilePlace::new`).
            taken("7i.pdf", "../../minutes.pdf"),
        ]),
        "only the names that fit"
    );
    for board in [
        serde_json::json!({"id": "b1"}),
        serde_json::json!({"id": "b1", "fileNames": ["7a.pdf"]}),
    ] {
        assert!(file_names_of(&board).is_empty(), "no names: {board}");
    }
    let by_dir = HashMap::from([("b1".to_owned(), names)]);
    assert_eq!(name_in(&by_dir, "b1", "7a.pdf"), "회의록.pdf");
    assert_eq!(name_in(&by_dir, "b1", "7b.pdf"), "7b.pdf", "left out");
    assert_eq!(
        name_in(&by_dir, "b2", "7a.pdf"),
        "7a.pdf",
        "another board's"
    );
}
