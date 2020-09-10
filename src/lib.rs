use serde::{Deserialize, Serialize};
use rusqlite::{Connection, Result, NO_PARAMS};
use std::str;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::Write;
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

// Returns map of playlsit name -> playlist
pub fn read_playlists(clemdbfile: &str) -> Option<HashMap<String, Playlist>> {
    let conn = Connection::open(clemdbfile).unwrap();

    // Query clementine 1.2.3 db for the ALL playlist data
    println!("Extracting clementing db data...");
    let mut stmt = conn
        .prepare("select playlists.name as playlist,  songs.title, songs.album, 
                  songs.artist, songs.track, songs.filename
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
            uri: relativeuri(String::from_utf8(row.get(5).unwrap()).unwrap()).to_string(), 
        })).expect("query_map failed");


    println!("Creating playlist data structures...");
    let mut all_playlists: HashMap<String,Playlist> =  HashMap::new();

    // Go through results making a data structure we can work with
    for song in song_iter {
        let songval = song.unwrap();
        let playlist_name: String = String::from(&songval.playlist);

        if exportable_playlist(&playlist_name) { 
//            songval.uri.insert_str(0,"music-library/USB/mediadrive/");
 
            let playlist_entry = all_playlists.entry(String::from(&playlist_name)).or_insert(
                Playlist {name: String::from(&playlist_name),
                          songs: Vec::new()});
            playlist_entry.songs.push(songval);
        }
    }

    Some(all_playlists)
}

pub fn extract_playlists(clemdbfile: &str) ->Result<()> {
    let conn = Connection::open(clemdbfile).unwrap();

    // Query clementine 1.2.3 db for the ALL playlist data
    println!("Extracting clementing db data...");
    let mut stmt = conn
        .prepare("select playlists.name as playlist,  songs.title, songs.album, 
                  songs.artist, songs.track, songs.filename
                  from playlist_items
                  join songs on songs._rowid_ = playlist_items.library_id
                  join playlists on playlist_items.playlist = playlists._rowid_")?;

    let song_iter = stmt
        .query_map(NO_PARAMS, |row| Ok(PlaylistItem {
            service: "mpd".to_owned(),
            playlist: row.get(0)?,
            title: row.get(1)?,
            artist: row.get(3)?,
            uri: relativeuri(String::from_utf8(row.get(5).unwrap()).unwrap()).to_string(), 
        }))?;


    println!("Creating playlist data structures...");
    let mut all_playlists: HashMap<String,Playlist> =  HashMap::new();

    let mut musiclist_file = File::create("musiclist.txt").unwrap();
    // Go through results making a data structure we can work with
    for song in song_iter {
        let mut songval = song.unwrap();
        let playlist_name: String = String::from(&songval.playlist);

        if exportable_playlist(&playlist_name) { 
            musiclist_file.write(&songval.uri.as_bytes()).unwrap();
            musiclist_file.write(b"\n").unwrap();

            songval.uri.insert_str(0,"music-library/USB/mediadrive/");
 
            let playlist_entry = all_playlists.entry(String::from(&playlist_name)).or_insert(
                Playlist {name: String::from(&playlist_name),
                          songs: Vec::new()});
            playlist_entry.songs.push(songval);
        }
    }
    musiclist_file.flush().unwrap();
    println!("Creating playlist files...");
    for (name, plist) in all_playlists {
       if exportable_playlist(&name) {
       // For each playlist create a file and output in a volumio readable format
           let pl_filename = String::from(&name) + ".vl";
           let plist_json = serde_json::to_string_pretty(&plist.songs).unwrap();
           fs::write(pl_filename, plist_json).expect("Unable to write playlist file");
       }
    }

    println!("Complete. Your playlists are *.vl");
    Ok(())
}


pub fn exportable_playlist(playlist: &str) -> bool {
    if playlist.starts_with("GV") {
        return true;
    }

    if playlist.starts_with("CV") {
        return true;
    }
    return false;
}

// Modifies the url to be a volumio relative so volumio can find the files
pub fn relativeuri<'a>(absuri: String) -> String { 
    let re = Regex::new(r"(.*[library_coco|library]/library)(.*)").unwrap();
    let uristr = percent_decode(absuri.as_bytes()).decode_utf8().unwrap();
    let result = re.replace(&uristr,"library${2}");

    let mut fixed_str = String::new();
    fixed_str.push_str(&*result);
    return fixed_str;
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
                }
            },
        }
    }
    
}
