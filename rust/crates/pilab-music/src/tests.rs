// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Index and scanner tests on generated WAV files (and on the demo albums when present).

use crate::scan::ScanCtl;
use crate::*;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use std::path::{Path, PathBuf};

struct Tmp(PathBuf);
impl Tmp {
    fn new(name: &str) -> Tmp {
        let p = std::env::temp_dir().join(format!("pilab-music-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A short 16-bit PCM WAV of silence.
fn wav(path: &Path, rate: u32, secs: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let frames = (rate as f32 * secs) as u32;
    let data_len = frames * 4;
    let mut b = vec![];
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.resize(b.len() + data_len as usize, 0);
    std::fs::write(path, b).unwrap();
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
    let mut out = vec![];
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&img).unwrap();
    out
}

fn tag(path: &Path, title: &str, artist: &str, album: &str, no: u32, extra: &[(ItemKey, &str)], pic: Option<Vec<u8>>) {
    let mut t = Tag::new(TagType::Id3v2);
    t.set_title(title.into());
    t.set_artist(artist.into());
    t.set_album(album.into());
    t.set_track(no);
    t.set_year(2024);
    t.set_genre("Electronic".into());
    for (k, v) in extra {
        t.insert_text(k.clone(), v.to_string());
    }
    if let Some(p) = pic {
        t.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Jpeg), None, p));
    }
    let mut id3: lofty::id3::v2::Id3v2Tag = t.into();
    // Picard writes the recording id as a UFID frame; generic tags do not, so add it here.
    if let Some((_, v)) = extra.iter().find(|(k, _)| *k == ItemKey::MusicBrainzRecordingId) {
        id3.insert(lofty::id3::v2::Frame::UniqueFileIdentifier(lofty::id3::v2::UniqueFileIdentifierFrame::new(
            "http://musicbrainz.org".into(),
            v.as_bytes().to_vec(),
        )));
    }
    id3.save_to_path(path, WriteOptions::default()).unwrap();
}

fn folder_src(id: &str, path: &Path, coll: &str) -> SourceCfg {
    SourceCfg {
        id: id.into(),
        kind: "folder".into(),
        path: path.to_string_lossy().into(),
        collection: coll.into(),
        name: id.into(),
        read_only: true,
        enabled: true,
        optional: false,
    }
}

/// Two tagged albums (one with folder art, one with embedded art and MusicBrainz ids),
/// an untagged "Artist - Album" folder, a recordings take and an M3U.
fn fixture(root: &Path) -> (Vec<SourceCfg>, ArtCache) {
    let m = root.join("music");
    let a1 = m.join("Ånne Beyoncé/First Light");
    for (i, t) in ["Dawn", "Noon", "Dusk"].iter().enumerate() {
        let p = a1.join(format!("0{} {t}.wav", i + 1));
        wav(&p, 48000, 0.5);
        tag(&p, t, "Ånne Beyoncé", "First Light", i as u32 + 1, &[(ItemKey::AlbumArtist, "Ånne Beyoncé")], None);
    }
    std::fs::write(a1.join("cover.jpg"), jpeg(640, 480)).unwrap();
    let a2 = m.join("various/Compilation");
    let mb = [(ItemKey::MusicBrainzReleaseId, "11111111-2222-3333-4444-555555555555"), (ItemKey::MusicBrainzRecordingId, "aaaaaaaa-0000-0000-0000-000000000001")];
    let p = a2.join("1.wav");
    wav(&p, 96000, 0.25);
    tag(&p, "Alpha", "Artist One", "Compilation", 1, &mb, Some(jpeg(320, 320)));
    let p = a2.join("2.wav");
    wav(&p, 96000, 0.25);
    tag(&p, "Beta", "Artist Two", "Compilation", 2, &mb[..1], None);
    let a3 = m.join("The Untagged - Raw Folder");
    wav(&a3.join("01 - First Thing.wav"), 44100, 0.2);
    wav(&a3.join("02. Second Thing.wav"), 44100, 0.2);
    std::fs::write(a3.join("notes.txt"), "not audio").unwrap();

    let r = root.join("recordings/pilab-rec-20261124-213005");
    wav(&r.join("pilab-rec-20261124-213005.wav"), 48000, 0.3);
    std::fs::write(
        r.join("record.toml"),
        "id = \"pilab-rec-20261124-213005\"\ncreated = \"2026-11-24T21:30:05\"\ncreated_unix = 1795000000\nduration_s = 0.3\nsource = \"synth\"\nscene = \"Pad study\"\nrole = \"take\"\nfiled = false\n",
    )
    .unwrap();
    let m3u = root.join("streams.m3u");
    std::fs::write(&m3u, "#EXTM3U\n#EXTINF:-1,Test Radio\nhttps://example.org/stream.mp3\nhttp://radio.example.net/live\n").unwrap();
    let mut urls = folder_src("radio", &m3u, "music");
    urls.kind = "urls".into();
    let srcs = vec![folder_src("music", &m, "music"), folder_src("recordings", &root.join("recordings"), "recordings"), urls];
    (srcs, ArtCache::new(root.join("art")))
}

#[test]
fn schema_has_v1_and_future_tables() {
    let ix = Index::open_memory().unwrap();
    let t = ix.tables().unwrap();
    for name in ["source", "holding", "artist", "release", "track", "take", "stream", "playlist", "playlist_item", "play_event", "override", "merge", "dup_candidate", "dup_decision", "fts"] {
        assert!(t.iter().any(|x| x == name), "missing table {name}: {t:?}");
    }
}

#[test]
fn scan_builds_albums_artists_tracks_takes_streams() {
    let tmp = Tmp::new("scan");
    let (srcs, art) = fixture(&tmp.0);
    let ix = Index::open(&tmp.0.join("db/library.sqlite")).unwrap();
    let rep = scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    assert_eq!(rep.albums, 3, "{rep:?}");
    assert_eq!(rep.tracks, 7);
    assert_eq!(rep.takes, 1);
    assert_eq!(rep.streams, 2);
    assert_eq!(rep.sources[0].added, 7);

    let albums = ix.albums("artist").unwrap();
    let titles: Vec<_> = albums.iter().map(|a| (a.artist.as_str(), a.title.as_str())).collect();
    assert!(titles.contains(&("Ånne Beyoncé", "First Light")), "{titles:?}");
    assert!(titles.contains(&("Various Artists", "Compilation")), "{titles:?}");
    assert!(titles.contains(&("The Untagged", "Raw Folder")), "{titles:?}");
    // Sort folds accents and ignores a leading "The ": anne < untagged < various.
    let pos = |t: &str| albums.iter().position(|a| a.title == t).unwrap();
    assert!(pos("First Light") < pos("Raw Folder") && pos("Raw Folder") < pos("Compilation"));

    // Album detail: track order, fallbacks, formats, MusicBrainz id.
    let fl = albums.iter().find(|a| a.title == "First Light").unwrap();
    let (_, tracks) = ix.album(fl.id).unwrap().unwrap();
    assert_eq!(tracks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Dawn", "Noon", "Dusk"]);
    assert_eq!(fl.format, "WAV 16/48");
    assert_eq!(fl.year, 2024);
    assert_eq!(fl.genre, "Electronic");
    let raw = albums.iter().find(|a| a.title == "Raw Folder").unwrap();
    let (_, tracks) = ix.album(raw.id).unwrap().unwrap();
    assert_eq!(tracks.iter().map(|t| (t.no, t.title.as_str())).collect::<Vec<_>>(), [(1, "First Thing"), (2, "Second Thing")]);
    let comp = albums.iter().find(|a| a.title == "Compilation").unwrap();
    assert_eq!(comp.mbid, "11111111-2222-3333-4444-555555555555");
    assert_eq!(comp.format, "WAV 16/96");
    let (_, ct) = ix.album(comp.id).unwrap().unwrap();
    assert_eq!(ct[0].mb_recording, "aaaaaaaa-0000-0000-0000-000000000001");
    assert_eq!(ct[0].artist, "Artist One");

    // Art: folder image for one album, embedded picture for the other, none for the third.
    assert!(!fl.art.is_empty() && art.best(&fl.art, 300).is_some() && art.best(&fl.art, 600).is_some());
    assert!(!comp.art.is_empty() && art.has(&comp.art));
    assert!(raw.art.is_empty());
    let thumb = image::open(art.best(&fl.art, 300).unwrap()).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (300, 300));

    // Artists list = album artists; artist detail lists appearances on other albums.
    let artists = ix.artists().unwrap();
    assert_eq!(artists.len(), 3, "{:?}", artists.iter().map(|a| &a.name).collect::<Vec<_>>());
    let all = ix.db.query_row("SELECT id FROM artist WHERE name = 'Artist One'", [], |r| r.get::<_, i64>(0)).unwrap();
    let (_, al, tr) = ix.artist(all).unwrap().unwrap();
    assert!(al.is_empty());
    assert_eq!(tr.len(), 1);

    // Recordings stay out of the music collection.
    let takes = ix.takes().unwrap();
    assert_eq!(takes[0].id, "pilab-rec-20261124-213005");
    assert_eq!(takes[0].session, "2026-11-24");
    assert_eq!(takes[0].title, "Take 21:30:05");
    assert_eq!(takes[0].scene, "Pad study");
    assert!(albums.iter().all(|a| a.collection == "music"));

    let st = ix.streams().unwrap();
    assert_eq!(st[0].name, "Test Radio");
    assert_eq!(st[1].name, "radio.example.net");
}

#[test]
fn rescan_is_incremental_and_ids_are_stable() {
    let tmp = Tmp::new("rescan");
    let (srcs, art) = fixture(&tmp.0);
    let ix = Index::open(&tmp.0.join("library.sqlite")).unwrap();
    scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    let before = ix.albums("title").unwrap();
    let pl = ix.playlist_create("Mix").unwrap();
    let dawn = ix.search("dawn", 5).unwrap().tracks[0].id;
    ix.playlist_add(pl, &[("track".into(), dawn.to_string(), String::new())]).unwrap();

    let rep = scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    assert_eq!((rep.sources[0].added, rep.sources[0].changed, rep.sources[0].removed), (0, 0, 0));
    assert_eq!(rep.art_made, 0, "thumbnails are cached");
    assert_eq!(ix.albums("title").unwrap(), before);
    assert_eq!(ix.playlist_items(pl).unwrap()[0].title, "Dawn");

    // Remove one file, retag another.
    let fl = tmp.0.join("music/Ånne Beyoncé/First Light");
    std::fs::remove_file(fl.join("03 Dusk.wav")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    tag(&fl.join("02 Noon.wav"), "Midday", "Ånne Beyoncé", "First Light", 2, &[(ItemKey::AlbumArtist, "Ånne Beyoncé")], None);
    let rep = scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    assert_eq!((rep.sources[0].added, rep.sources[0].changed, rep.sources[0].removed), (0, 1, 1));
    assert_eq!(rep.tracks, 6);
    assert!(ix.search("midday", 5).unwrap().tracks.len() == 1);
    assert!(ix.search("dusk", 5).unwrap().tracks.is_empty());
    // Same release id after the change; the playlist still resolves.
    let after = ix.albums("title").unwrap();
    assert_eq!(after.iter().find(|a| a.title == "First Light").unwrap().id, before.iter().find(|a| a.title == "First Light").unwrap().id);
    assert!(!ix.playlist_items(pl).unwrap()[0].missing);
}

#[test]
fn offline_optional_source_keeps_library_and_removed_source_is_dropped() {
    let tmp = Tmp::new("offline");
    let (mut srcs, art) = fixture(&tmp.0);
    let ix = Index::open_memory().unwrap();
    scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    srcs[0].path = tmp.0.join("unplugged").to_string_lossy().into();
    srcs[0].optional = true;
    let rep = scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    assert_eq!(rep.sources[0].state, "offline");
    assert_eq!(rep.tracks, 7, "offline drive keeps its index");
    srcs.remove(0);
    let rep = scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    assert_eq!(rep.tracks, 0);
    assert_eq!(rep.takes, 1);
}

#[test]
fn search_is_prefix_and_diacritic_insensitive() {
    let tmp = Tmp::new("search");
    let (srcs, art) = fixture(&tmp.0);
    let ix = Index::open_memory().unwrap();
    scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    let r = ix.search("beyon", 10).unwrap();
    assert_eq!(r.artists.len(), 1);
    assert_eq!(r.albums.len(), 1);
    assert_eq!(r.tracks.len(), 3);
    let r = ix.search("anne first", 10).unwrap();
    assert_eq!(r.albums[0].title, "First Light");
    assert_eq!(ix.search("pad", 10).unwrap().takes.len(), 1);
    assert_eq!(ix.search("test radio", 10).unwrap().streams.len(), 1);
    assert!(ix.search("\"*()", 10).unwrap().tracks.is_empty());
    assert!(ix.search("zzzz", 10).unwrap().albums.is_empty());
}

#[test]
fn playlists_crud_and_playables() {
    let tmp = Tmp::new("pl");
    let (srcs, art) = fixture(&tmp.0);
    let ix = Index::open_memory().unwrap();
    scan(&ix, &srcs, &art, &ScanCtl::default()).unwrap();
    let tr: Vec<i64> = ix.tracks(100, 0).unwrap().iter().map(|t| t.id).collect();
    let p = ix.playlist_create("  Road trip ").unwrap();
    assert!(ix.playlist_create("").is_err());
    let items = vec![
        ("track".to_string(), tr[0].to_string(), String::new()),
        ("take".to_string(), "pilab-rec-20261124-213005".to_string(), String::new()),
        ("url".to_string(), "https://example.org/stream.mp3".to_string(), "Radio".to_string()),
        ("track".to_string(), tr[1].to_string(), String::new()),
    ];
    assert_eq!(ix.playlist_add(p, &items).unwrap(), 4);
    assert!(ix.playlist_add(p, &[("url".into(), "javascript:alert(1)".into(), String::new())]).is_err());
    assert!(ix.playlist_add(p, &[("track".into(), "999999".into(), String::new())]).is_err());
    let pls = ix.playlists().unwrap();
    assert_eq!((pls[0].name.as_str(), pls[0].count), ("Road trip", 4));
    assert!(ix.playlist_move(p, 3, 0).unwrap());
    let it = ix.playlist_items(p).unwrap();
    assert_eq!(it.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), ["track", "track", "take", "stream"]);
    assert_eq!(it[0].track_id, Some(tr[1]));
    assert!(ix.playlist_remove(p, 2).unwrap());
    assert_eq!(ix.playlist_items(p).unwrap().iter().map(|i| i.pos).collect::<Vec<_>>(), [0, 1, 2]);
    let pl = ix.playlist_playables(p).unwrap();
    assert_eq!(pl.len(), 3);
    assert!(pl[0].uri.starts_with("file:///"));
    assert_eq!(pl[2].uri, "https://example.org/stream.mp3");
    // Queue lookups from the URIs MPD reports back.
    let back = ix.lookup_uri(&pl[0].uri).unwrap().unwrap();
    assert_eq!(back.id, tr[1].to_string());
    let path = index::path_from_uri(&pl[0].uri).unwrap();
    assert_eq!(ix.lookup_uri(&path).unwrap().unwrap().kind, "track");
    let take = ix.take_playable("pilab-rec-20261124-213005").unwrap().unwrap();
    assert_eq!(ix.lookup_uri(&take.uri).unwrap().unwrap().kind, "take");
    assert!(ix.playlist_rename(p, "Renamed").unwrap());
    assert!(ix.playlist_delete(p).unwrap());
    assert!(ix.playlists().unwrap().is_empty());
}

#[test]
fn uri_round_trip_and_filename_rules() {
    let p = "/home/x/Music/Ånne Beyoncé/01 #1 100% (live).flac";
    let u = index::file_uri(p);
    assert_eq!(u, format!("file://{p}"));
    assert_eq!(index::path_from_uri("https://x/y"), None);
    assert_eq!(index::path_from_uri(&u).unwrap(), p);
    use tags::{album_from_folder, title_from_filename};
    assert_eq!(title_from_filename(Path::new("03 - Title.mp3")), (3, "Title".into()));
    assert_eq!(title_from_filename(Path::new("1.RailwayToNowhere.mp3")), (1, "RailwayToNowhere".into()));
    assert_eq!(title_from_filename(Path::new("Ballade no. 1 - Op. 23.ogg")), (0, "Ballade no. 1 - Op. 23".into()));
    assert_eq!(title_from_filename(Path::new("1999.flac")), (0, "1999".into()));
    assert_eq!(album_from_folder(Path::new("/m/Chopin - Ballades (Musopen)")), (Some("Chopin".into()), "Ballades (Musopen)".into()));
    assert_eq!(album_from_folder(Path::new("/m/Loose")), (None, "Loose".into()));
    assert_eq!(sort_key("The Beatles"), "beatles");
    assert_eq!(stable_id(&["a", "b"]), stable_id(&["a", "b"]));
    assert!((0..1000).all(|i| stable_id(&[&i.to_string()]) < (1i64 << 53)), "ids stay exact in JavaScript");
    assert_ne!(stable_id(&["ab", ""]), stable_id(&["a", "b"]));
}

#[test]
fn m3u_and_sources_config() {
    let e = m3u::parse("\u{feff}#EXTM3U\n#EXTINF:-1,Groove\nhttps://a.example/x\n# comment\nftp://nope\nhttp://b.example:8000/live\n");
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].name, "Groove");
    assert_eq!(e[1].name, "b.example:8000");
    assert_eq!(m3u::parse(&m3u::render(&e)), e);
    let d = Sources::defaults("~/.local/share/pilab/recordings");
    assert!(d.get("recordings").unwrap().collection == "recordings");
    let back: Sources = toml::from_str(&d.to_toml()).unwrap();
    assert_eq!(back.source, d.source);
    let bad: Sources = toml::from_str("[[source]]\nid = \"x y\"\npath = \"/\"\n").unwrap();
    assert!(bad.source[0].validate().is_err());
}

#[test]
fn pause_holds_the_scan() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let tmp = Tmp::new("pause");
    let (srcs, art) = fixture(&tmp.0);
    let ix = Index::open_memory().unwrap();
    let held = Arc::new(AtomicBool::new(true));
    let h2 = held.clone();
    let ctl = ScanCtl { pause: Some(Box::new(move || h2.load(Ordering::Relaxed))), ..Default::default() };
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        held.store(false, Ordering::Relaxed);
    });
    let t0 = std::time::Instant::now();
    scan(&ix, &srcs, &art, &ctl).unwrap();
    assert!(t0.elapsed().as_millis() >= 600);
    t.join().unwrap();
}

/// The real demo albums, when downloaded on this machine (skipped otherwise).
#[test]
fn demo_albums_if_present() {
    let Ok(dir) = std::env::var("PILAB_MUSIC_DEMO") else { return };
    let tmp = Tmp::new("demo");
    let ix = Index::open_memory().unwrap();
    let rep = scan(&ix, &[folder_src("demo", Path::new(&dir), "music")], &ArtCache::new(tmp.0.join("art")), &ScanCtl::default()).unwrap();
    eprintln!("{}", serde_json::to_string_pretty(&ix.albums("artist").unwrap()).unwrap());
    assert!(rep.albums >= 3, "{rep:?}");
    assert!(ix.albums("artist").unwrap().iter().all(|a| !a.title.is_empty()));
}

#[test]
fn two_disc_folders_without_album_artist_are_one_release() {
    let t = Tmp::new("discs");
    let m = t.0.join("music/Box Set");
    for (d, names) in [("Disc 1", ["One", "Two"]), ("Disc 2", ["Three", "Four"])] {
        for (i, n) in names.iter().enumerate() {
            let p = m.join(d).join(format!("0{} {n}.wav", i + 1));
            wav(&p, 44100, 0.2);
            tag(&p, n, "Brian Eno", "Box Set", i as u32 + 1, &[], None);
        }
    }
    let ix = Index::open_memory().unwrap();
    let srcs = vec![folder_src("music", &t.0.join("music"), "music")];
    let rep = scan::scan(&ix, &srcs, &ArtCache::new(t.0.join("art")), &ScanCtl::default()).unwrap();
    assert_eq!(rep.albums, 1);
    assert_eq!(rep.tracks, 4);
}
