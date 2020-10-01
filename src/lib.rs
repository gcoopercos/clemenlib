use serde::{Deserialize, Serialize};
use rusqlite::{Connection, Result, NO_PARAMS};
use std::str;
use std::fmt::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::Write;
use std::io::BufWriter;
use std::path::PathBuf;
use std::path::Path;
use percent_encoding::percent_decode;

use std::net::TcpStream;
use ssh2::Session;

#[derive(Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub songs: Vec<PlaylistItem>
}


#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistItem {
    #[serde(skip_serializing)]
    pub playlist: String,
    pub service: String,
    pub title: String,
    pub artist: String,
    pub uri: String,
    pub length: i64,
}

pub fn copy_from_local_clem(tempfile: &str) -> Result<()> {
    let clemfile = get_local_clementine().expect("Problem determining clementine path");
    fs::copy(clemfile, tempfile).expect("Problem copying from local clementine");
    Ok(())
}

pub fn get_local_clementine() -> Result<PathBuf> {
    let mut clemfile = PathBuf::new();
    match dirs::home_dir() {
        None => {},
        Some(homedir) => {
            clemfile.push(homedir);
        },
    }
    clemfile.push(".config");
    clemfile.push("Clementine");
    clemfile.push("clementine.db");
    
    Ok(clemfile)
}

pub fn copy_from_remote_clem(hostportconfig: &str,
                             username: &str,
                             tempfile: &str) -> Result<()> {

    // Connect to the local SSH server
    // let tcp = TcpStream::connect("127.0.0.1:22").unwrap();
    let tcp = TcpStream::connect(&hostportconfig).unwrap();
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();

    // Might want to use "userauth_pubkey_file(..)
    sess.userauth_agent(username).unwrap();

    let remote_file_name = format!("{}{}{}", "/home/", username, "/.config/Clementine/clementine.db");
    
    let (mut remote_file, stat) = sess.scp_recv(Path::new(&remote_file_name)).unwrap();
    println!("remote file size: {}", stat.size());
    let mut contents = Vec::new();
    remote_file.read_to_end(&mut contents).unwrap();
    let mut destfile = File::create(tempfile).expect("Unable to create file");
    destfile.write_all(&contents).expect("Unable to write file");
    Ok(())
}

pub fn copy_playlist_to_remote(hostportconfig: &str,
                               username: &str,
                               local_file: &str,
                               remote_file: &str) -> Result<()> {

    // Connect to the local SSH server
    // let tcp = TcpStream::connect("127.0.0.1:22").unwrap();
    let tcp = TcpStream::connect(&hostportconfig).unwrap();
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();

    // Might want to use "userauth_pubkey_file(..)
    sess.userauth_agent(username).unwrap();

    //    let remote_file_name = format!("{}{}{}", "/home/", username, "/.config/Clementine/clementine.db");
    let data = fs::read(local_file).expect("Unable to read file");

    println!("Sending file: {}, length = {}", remote_file, data.len());
    let mut remote_file =
        sess.scp_send(Path::new(remote_file),
                      0o644, data.len() as u64, None).unwrap();

    remote_file.write_all(&data).unwrap();

    Ok(())
}

pub fn export_m3u(playlist: &Playlist, prefix: &str) -> Result<()>{
    let dfilename = String::from(&playlist.name) + ".m3u";
    let destfile =
        File::create(&dfilename).expect("Unable to create m3u file");

    let mut destfile =
        BufWriter::new(destfile);
    
    for song in &playlist.songs {
        // let mut comment_line = String::from(format!("#EXTINF:{},{}",
        //                                             song.length,
        //                                             song.title));
        println!("{}", song.uri);
        writeln!(destfile,"#EXTINF:{},{}",song.length,song.title).unwrap();
        let proper_loc = prefix.to_owned() + &song.uri;
        writeln!(destfile,"{}",proper_loc).unwrap();
        destfile.flush().unwrap();
    }
    
    Ok(())
}

pub fn export_volumio(playlist: &Playlist) -> Result<()>{
    let dfilename = String::from(&playlist.name) + ".vl";

    let plist_json = serde_json::to_string_pretty(&playlist.songs).unwrap();
    fs::write(&dfilename, plist_json).expect("Unable to write volumio file");
    println!("Playlist written: {}", dfilename);
    Ok(())
}

pub fn read_raw_playlist(clemdbfile: &str,
                         playlist_name: &str) -> Option<Playlist> {

    let conn = Connection::open(clemdbfile).unwrap();

    // Query clementine 1.2.3 db for the ALL playlist data
    let mut stmt = conn
        .prepare("
select playlists.name as playlist, 
    items.title, items.artist, items.filename, items.length
from playlists
join playlist_items items on items.playlist = playlists._rowid_
where playlists.name = ?").unwrap();


    let song_iter = stmt
        .query_map(&[&playlist_name], |row| Ok(PlaylistItem {
            service: "mpd".to_owned(),
            playlist: row.get(0).expect("playlist retrieval problem"),
            title: row.get(1).expect("title retrieval problem"),
            artist: row.get(2).expect("artist retrieval problem"),
            // uri: relativeuri(&library_root,
            //                  String::from_utf8(row.get(5).unwrap()).unwrap()),
            uri: String::from_utf8(row.get(3).unwrap()).unwrap(),
            length: row.get(4).expect("length retrieval problem"),
        })).expect("query_map failed");


    println!("Creating playlist data structures from clementine database...");

    // Go through results making a data structure we can work with
    let mut playlist = Playlist{name: String::from(playlist_name),
                                songs: Vec::new()};
    
    for song in song_iter {
        let songval = song.unwrap();
        // let playlist_name: String = String::from(&songval.playlist);
        // let playlist_entry = all_playlists.entry(String::from(&playlist_name)).or_insert(
        //     Playlist {name: String::from(&playlist_name),
        //               songs: Vec::new()});
        // if let Some(pfx) = &playlist_entry_prefix {
        //     songval.uri.insert_str(0,pfx);
        // }
        playlist.songs.push(songval);
    }

    Some(playlist)
    
}

// Returns map of playlsit name -> playlist
pub fn read_playlists(library_root: &str,
                      clemdbfile: &str,
                      playlist_entry_prefix: Option<&str>) -> Option<HashMap<String, Playlist>> {
    let conn = Connection::open(clemdbfile).unwrap();

    // Query clementine 1.2.3 db for the ALL playlist data
    let mut stmt = conn
        .prepare("select playlists.name as playlist,  songs.title, songs.album, 
                  songs.artist, songs.track, songs.filename, songs.length
                  from playlist_items
                  join songs on songs._rowid_ = playlist_items.library_id
                  join playlists on playlist_items.playlist = playlists._rowid_")
        .expect("Clementine DB Query failed");

    let song_iter = stmt
        .query_map(NO_PARAMS, |row| Ok(PlaylistItem {
            service: "mpd".to_owned(),
            playlist: row.get(0).expect("playlist retrieval problem"),
            title: row.get(1).expect("title retrieval problem"),
            artist: row.get(3).expect("artist retrieval problem"),
            uri: relativeuri(&library_root,
                             String::from_utf8(row.get(5).unwrap()).unwrap()),
            length: row.get(6).expect("length retrieval problem"),
        })).expect("query_map failed");


    println!("Creating playlist data structures from clementine database...");
    let mut all_playlists: HashMap<String,Playlist> =  HashMap::new();

    // Go through results making a data structure we can work with
    for song in song_iter {
        let mut songval = song.unwrap();
        let playlist_name: String = String::from(&songval.playlist);
        let playlist_entry = all_playlists.entry(String::from(&playlist_name)).or_insert(
            Playlist {name: String::from(&playlist_name),
                      songs: Vec::new()});
        if let Some(pfx) = &playlist_entry_prefix {
            songval.uri.insert_str(0,pfx);
        }
        playlist_entry.songs.push(songval);
    }

    Some(all_playlists)
}

// Modifies the url to be a volumio relative so volumio can find the files
pub fn relativeuri(library_root: &str, absuri: String) -> String { 
    let re = Regex::new(&(r"(.*".to_string()+library_root+")(.*)")).unwrap();
    // println!("re: {}", re);
    let uristr = percent_decode(absuri.as_bytes()).decode_utf8().unwrap();
    let result = re.replace(&uristr,"${2}");

    let mut fixed_str = String::new();
    fixed_str.push_str(&*result);
    fixed_str
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relativeuri() {
        assert_eq!(
            "library1/aerosmith/greatest_hits/back_in_the_saddle.mp3",
            relativeuri(String::from("file:///home/music/library_coco/library1/aerosmith/greatest_hits/back_in_the_saddle.mp3"))
            );
        assert_eq!(
             "library1/aerosmith/greatest_hits/back_in_the_saddle.mp3",
            relativeuri(String::from("file:///mnt/storage1/music/library/library1/aerosmith/greatest_hits/back_in_the_saddle.mp3"))
            );
    }

    // #[test]
    // fn test_copy_from_local_clem() {
    //     copy_from_local_clem("/tmp/clemtest.db");
    // }
    // #[test]
    // fn test_copy_from_remote_clem() {
    //     // copy_from_remote_clem("saturn.local:22",
    //     copy_from_remote_clem("192.168.1.54:22",
    //                           "colleen",
    //                           "/tmp/clemtestcol.db");
    // }


    #[test]
    fn test_read_playlists() {
        match read_playlists("/tmp/clemtest.db") {
            None => {},
            Some(playlists) => {
                for (name, plist) in playlists {
                    println!("Playlist: {}", name);
//                    println!("{}", serde_json::to_string_pretty(&plist.songs).unwrap());
                    if name == "GV-JimCathy" {
                        export_m3u(&plist, "/home/music/library_coco/");
                    }
                }
            },
        }
    }
    
}
