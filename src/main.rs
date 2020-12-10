/*
    This file is part of Oku.

    Oku is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    Oku is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with Oku.  If not, see <https://www.gnu.org/licenses/>.
*/

use percent_encoding::percent_decode_str;
use async_recursion::async_recursion;
use directories_next::ProjectDirs;
use gtk::Inhibit;

use futures::TryStreamExt;
use gtk::prelude::BuilderExtManual;
use gtk::ButtonExt;
use gtk::EntryExt;
use gtk::WidgetExt;
use ipfs_api::IpfsClient;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use webkit2gtk::WebViewExt;
use glib::clone;
use url::{Url, Position};

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref PROJECT_DIRECTORIES: ProjectDirs =
        ProjectDirs::from("org", "Emil Sayahi", "Oku").unwrap();
}

fn main() {
    let cache_directory = PROJECT_DIRECTORIES.cache_dir().to_str().unwrap();
    let client = IpfsClient::default();

    if gtk::init().is_err() {
        println!("Failed to initialize GTK.");
        return;
    }
    let glade_src = include_str!("window.glade");
    let web_kit = webkit2gtk::WebViewBuilder::new();
    web_kit.build();
    let builder = gtk::Builder::from_string(glade_src);

    let window: gtk::Window = builder.get_object("window").unwrap();
    let go_button: gtk::Button = builder.get_object("go_button").unwrap();
    let nav_entry: gtk::Entry = builder.get_object("nav_entry").unwrap();
    let web_view: webkit2gtk::WebView = builder.get_object("webkit_view").unwrap();

    go_button.connect_clicked(move |_go_button| {
        let url = Url::parse(&nav_entry.get_text().to_string()).unwrap();
        if url.scheme() == "ipfs"
        {
            let hash = &url[Position::BeforeHost..];
            let decoded_hash = percent_decode_str(&hash.to_owned()).decode_utf8().unwrap().to_string();
            let local_directory = format!("{}/{}", cache_directory, decoded_hash);

            get_from_hash(client.clone(), decoded_hash, local_directory.clone());

            web_view.load_uri(&format!("file://{}", &local_directory));

            println!("Loading: file://{} … \n", &local_directory);
        }
        else
        {
            web_view.load_uri(&nav_entry.get_text().to_string());
        }

        web_view.connect_load_changed(clone!(@weak web_view, @weak nav_entry => move |_, _| {
            nav_entry.set_text(&web_view.get_uri().unwrap().to_string().replacen(&format!("file://{}/", cache_directory), "ipfs://", 1));
        }));
    });

    window.show_all();
    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });
    gtk::main();
}

fn get_from_hash(client: IpfsClient, hash: String, local_directory: String) {
    let mut hierarchy = HashMap::new();
    hierarchy.insert(hash.to_owned(), local_directory.to_owned());
    let mut sys = actix_rt::System::new("Oku IPFS System");
    sys.block_on(async move {
        /*ipfs_download_directory(
            &client,
            local_directory.to_owned(),
            hash.to_owned(),
            hierarchy,
        )
        .await;*/
        ipfs_download_file(
            &client,
            hash.to_owned(),
            local_directory.to_owned(),
        )
        .await;
        println!("Requesting: {} (local: {}) … \n", hash.to_owned(), local_directory.to_owned());
    });
}

async fn ipfs_download_file(client: &IpfsClient, file_hash: String, file_path: String) {
    match client
        .cat(&file_hash)
        .map_ok(|chunk| chunk.to_vec())
        .try_concat()
        .await
    {
        Ok(res) => {
            println!("\nWriting: {} ({}) … \n", file_path, file_hash);
            fs::create_dir_all(Path::new(&file_path[..]).parent().unwrap()).unwrap();
            fs::write(file_path, &res).unwrap();
        }
        Err(e) => eprintln!(
            "\nFailed to obtain file: {} ({})\nError: {:#?}\n",
            file_path, file_hash, e
        ),
    }
}

#[async_recursion(?Send)]
async fn ipfs_download_directory(
    client: &IpfsClient,
    directory: String,
    directory_hash: String,
    mut hierarchy: HashMap<String, String>,
) {
    hierarchy.insert(directory_hash.clone(), directory.clone());
    let directory_object = client.file_ls(&directory_hash).await.unwrap().objects;
    for object in directory_object {
        for link in object.1.links {
            let link_type = &link.typ.clone().unwrap();
            match link_type.as_str() {
                "Directory" => {
                    let sub_directory =
                        format!("{}/{}", hierarchy.get(&directory_hash).unwrap(), link.name);
                    ipfs_download_directory(
                        &client,
                        sub_directory.to_owned(),
                        link.hash.clone(),
                        hierarchy.clone(),
                    )
                    .await;
                    hierarchy.insert(link.hash, sub_directory);
                }
                "File" => {
                    ipfs_download_file(
                        &client,
                        link.hash,
                        format!("{}/{}", hierarchy.get(&directory_hash).unwrap(), link.name),
                    )
                    .await;
                }
                _ => {}
            }
        }
    }
}
