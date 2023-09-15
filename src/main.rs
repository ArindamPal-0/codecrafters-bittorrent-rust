use serde::{Deserialize, Serialize};
use serde_bencode;
use serde_bytes::ByteBuf;
use serde_json;
use sha1::{Digest, Sha1};
use std::env;

#[derive(Serialize, Deserialize, Debug)]
struct Info {
    name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length")]
    piece_length: i64,
    length: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TorrentMetadata {
    announce: String,
    info: Info,
}

/// Returns the `end_index` of the next datatype
///
/// # Arguments
/// * `encoded_value` - string slice for parsing the next datatype
/// * `start_index` - `usize` for inclusive start index
/// * `end_index` - `usize` for inclusive end index
fn get_end_index_for_next_datatype(
    encoded_value: &str,
    start_index: usize,
    end_index: usize,
) -> usize {
    let mut end_index = end_index;

    let encoded_value_range = &encoded_value[start_index..=end_index];

    // Next String data
    if encoded_value_range.chars().next().unwrap().is_digit(10) {
        let colon_index = encoded_value_range
            .find(':')
            .expect(": is missing, string should be <size>:<string>");
        let size = encoded_value_range[..colon_index]
            .parse::<i64>()
            .expect("size is not a number in <size>:<string>") as usize;
        end_index = colon_index + size;
    }
    // Next Int data
    else if encoded_value_range.starts_with("i") {
        end_index = encoded_value_range.find('e').unwrap();
    }
    // Next List data
    else if encoded_value_range.starts_with("l") {
        let mut next_index = 1 as usize;
        loop {
            if encoded_value_range
                .get(next_index..)
                .unwrap()
                .starts_with("e")
            {
                break;
            }

            next_index =
                get_end_index_for_next_datatype(encoded_value, start_index + next_index, end_index)
                    + 1
                    - start_index;
        }

        end_index = next_index;
    }
    // Next Dict data
    else if encoded_value_range.starts_with("d") {
        let mut next_index = 1 as usize;
        loop {
            if encoded_value_range
                .get(next_index..)
                .unwrap()
                .starts_with("e")
            {
                break;
            }

            next_index =
                get_end_index_for_next_datatype(encoded_value, start_index + next_index, end_index)
                    + 1
                    - start_index;
        }

        end_index = next_index;
    }

    return start_index + end_index;
}

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> serde_json::Value {
    // If encoded_value starts with a digit, it's a string
    if encoded_value.chars().next().unwrap().is_digit(10) {
        // Example: "5:hello" -> "hello"
        let colon_index = encoded_value
            .find(':')
            .expect(": is missing, string should be <size>:<string>");
        // let number_string = &encoded_value[..colon_index];
        // let string_length = number_string.parse::<i64>().unwrap();
        // let string = &encoded_value[colon_index + 1..colon_index + 1 + string_length as usize];
        let num_string = &encoded_value[colon_index + 1..];
        return serde_json::Value::String(num_string.to_string());
    // If encoded_value starts with 'i' and ends with 'e', it's a number
    } else if encoded_value.starts_with("i") && encoded_value.ends_with("e") {
        // Example: "i52e" -> 52
        // let e_index = encoded_value.find('e').unwrap();
        // let number_string = &encoded_value[1..e_index];
        // let number = number_string.parse::<i64>().unwrap();
        let number = encoded_value
            .strip_prefix("i")
            .unwrap()
            .strip_suffix("e")
            .unwrap()
            .parse::<i64>()
            .unwrap();
        return serde_json::Value::Number(number.into());
    // If encoded_value starts with 'l' and ends with 'e', it's a list
    } else if encoded_value.starts_with("l") && encoded_value.ends_with("e") {
        // Example "l5:helloei2ee" -> ["hello", 2]
        let mut values = Vec::new();

        let mut next_first_index = 1 as usize;
        let end_index = encoded_value.len() - 1;
        while next_first_index < end_index {
            let next_end_index =
                get_end_index_for_next_datatype(encoded_value, next_first_index, end_index);

            let value = decode_bencoded_value(&encoded_value[next_first_index..=next_end_index]);

            values.push(value);

            next_first_index = next_end_index + 1;
        }

        return serde_json::Value::Array(values);
    // If encoded_value starts with 'd' and ends with 'e', it's a dictionary
    } else if encoded_value.starts_with("d") && encoded_value.ends_with("e") {
        // Example "d5:hello5:world3:fooi32ee" -> {"hello":"world","foo":3}
        let mut map = serde_json::Map::new();

        let mut next_first_index = 1 as usize;
        let end_index = encoded_value.len() - 1;
        while next_first_index < end_index {
            let next_end_index =
                get_end_index_for_next_datatype(encoded_value, next_first_index, end_index);

            let key = decode_bencoded_value(&encoded_value[next_first_index..=next_end_index]);
            if !key.is_string() {
                panic!("key should be a string not a {:?}", key);
            }

            // let key = key.to_string().trim_matches('\"').to_string();
            let key = key.as_str().unwrap().to_string();

            next_first_index = next_end_index + 1;
            if next_first_index >= end_index {
                panic!("only key but no value is present, key-value pair should be present");
            }

            let next_end_index =
                get_end_index_for_next_datatype(encoded_value, next_first_index, end_index);

            let value = decode_bencoded_value(&encoded_value[next_first_index..=next_end_index]);

            map.insert(key, value);

            next_first_index = next_end_index + 1;
        }

        return serde_json::Value::Object(map);
    } else {
        panic!("Unhandled encoded value: {}", encoded_value)
    }
}

fn transform_bencode_to_json(value: &serde_bencode::value::Value) -> serde_json::Value {
    match value {
        serde_bencode::value::Value::Bytes(b) => {
            if let Ok(s) = String::from_utf8(b.clone()) {
                serde_json::Value::String(s)
            } else {
                // serde_bytes::ByteBuf::from(b.clone())
                serde_json::Value::Null
            }
            // serde_json::Value::String(String::from_utf8(b.clone()).unwrap())
        }
        serde_bencode::value::Value::Int(i) => serde_json::Value::Number((*i).into()),
        serde_bencode::value::Value::List(l) => {
            let values = l.iter().map(transform_bencode_to_json).collect();
            serde_json::Value::Array(values)
        }
        serde_bencode::value::Value::Dict(d) => {
            let map = d
                .iter()
                .filter_map(|(key, value)| {
                    String::from_utf8(key.clone())
                        .ok()
                        .map(|key_str| (key_str, transform_bencode_to_json(value)))
                })
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

#[allow(dead_code)]
fn decode_bencoded_value_serde_bencode(encoded_value: &[u8]) -> serde_json::Value {
    let value: serde_bencode::value::Value = serde_bencode::from_bytes(encoded_value).unwrap();
    return transform_bencode_to_json(&value);
}

// #[allow(dead_code)]
// fn get_torrent_info(torrent_file_contents: &[u8]) {
//     let dictionary = decode_bencoded_value_serde_bencode(torrent_file_contents);
//     println!("torrent_metadata: {:?}", dictionary);
// }

// Usage: your_bittorrent.sh decode "<encoded_value>"
fn main() {
    let args: Vec<String> = env::args().collect();
    // let command = args.get(1).expect("No command specified");
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];

        // uses self-made bencode parser
        // let decoded_value = decode_bencoded_value(encoded_value);

        // uses serde_bencode for parsing
        let decoded_value = decode_bencoded_value_serde_bencode(encoded_value.as_bytes());

        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        let torrent_file_path = &args[2];

        let file_contents =
            std::fs::read(torrent_file_path).expect("Not able to read torrent file.");

        // let decoded_value = decode_bencoded_value_serde_bencode(&file_contents);

        // println!("{}", decoded_value.to_string());

        let torrent_metadata =
            serde_bencode::from_bytes::<TorrentMetadata>(&file_contents).unwrap();
        println!("Tracker URL: {}", torrent_metadata.announce);
        // println!("Info: {:?}", torrent_metadata.info);
        println!("Length: {}", torrent_metadata.info.length.unwrap());

        // let decoded_value = decode_bencoded_value_serde_bencode(&file_contents);
        // println!("decoded_value: {}", decoded_value);

        let info_encoded_value = serde_bencode::to_bytes(&torrent_metadata.info).unwrap();

        let mut hasher = Sha1::new();
        hasher.update(info_encoded_value);
        let info_hash = hasher.finalize();

        println!("Info Hash: {:x}", info_hash);

        println!("Piece Length: {}", torrent_metadata.info.piece_length);

        println!("Piece Hashes:");
        for piece_hash in torrent_metadata.info.pieces.chunks(20) {
            let hash: Vec<_> = piece_hash
                .iter()
                .map(|byte| format!("{:02x}", byte))
                .collect();
            println!("{}", hash.join(""));
        }
    } else {
        println!("unknown command: {}", args[1]);
    }
}
