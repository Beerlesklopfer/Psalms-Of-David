// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prints every tag lofty finds in a file, per tag type. Used to see how each
//! format stores its fields: `cargo run --example dump_tags -- file...`
use lofty::prelude::*;

fn main() {
    for path in std::env::args().skip(1) {
        println!("== {path}");
        let tagged = match lofty::read_from_path(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("   error: {e}");
                continue;
            }
        };
        println!("   file type {:?}, duration {:?}", tagged.file_type(), tagged.properties().duration());
        for tag in tagged.tags() {
            println!("   tag type {:?}", tag.tag_type());
            for item in tag.items() {
                println!("      {:?} = {:?}", item.key(), item.value());
            }
        }
    }
}
