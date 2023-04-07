use clap::{Arg, Command};
use openai::{
    chat::{self, ChatCompletionMessage, ChatCompletionMessageRole},
    set_key,
};
use reqwest::{Client, ClientBuilder};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error, sync::Arc};

// Config for the cli
#[derive(Serialize, Deserialize)]
struct MyConfig {
    openai_key: String,
    students: Vec<Student>,
}

/// `MyConfig` implements `Default`
impl ::std::default::Default for MyConfig {
    fn default() -> Self {
        Self {
            openai_key: "".into(),
            students: Default::default(),
        }
    }
}

// Struct to store cuname and pass for each student
#[derive(Serialize, Deserialize)]
struct Student {
    cuname: String,
    pass: String,
}

// Send an openai api request to get the solution code
async fn solve_prob(client: &Client, prob: &str) -> String {
    // Parse the html of the problem page
    let res = client
        .get(format!("https://codingbat.com/prob/{}", prob))
        .send()
        .await
        .expect("Failed to send request");
    let document = Html::parse_document(&res.text().await.expect("Failed to get text"));
    let prob_sel = Selector::parse(".max2").expect("Failed to parse selector");
    let excode_sel = Selector::parse("#ace_div").expect("Failed to parse selector");

    let problem = document
        .select(&prob_sel)
        .next()
        .expect("Could not parse problem")
        .text()
        .collect::<Vec<_>>()[0]
        .trim();
    let excode = document
        .select(&excode_sel)
        .next()
        .expect("Could not parse example code")
        .text()
        .collect::<Vec<_>>()[0]
        .trim();

    // Build the starting message
    let sys_message = ChatCompletionMessage{
        role: ChatCompletionMessageRole::System,
        content: "Solve the provided problem by editing the provided java method. Only respond with the unformatted code and nothing else.".to_string(),
        name: None
    };

    // Build the problem message
    let user_message = ChatCompletionMessage {
        role: chat::ChatCompletionMessageRole::User,
        content: format!("{}\n{}", problem, excode),
        name: None,
    };

    // Send the request to the openai api
    chat::ChatCompletion::builder("gpt-3.5-turbo", vec![sys_message, user_message])
        .create()
        .await
        .unwrap()
        .unwrap()
        .choices
        .first()
        .unwrap()
        .message
        .content
        .clone()
}

// Send a post request to run the code
async fn run_code(client: &Client, prob: &str, code: &str, cuname: &str) {
    let mut form_data = HashMap::new();
    form_data.insert("id", prob);
    form_data.insert("code", code);
    form_data.insert("cuname", cuname);

    client
        .post("https://codingbat.com/run")
        .form(&form_data)
        .send()
        .await
        .expect("Failed to send run request");
}

// Logs the user in
async fn login(client: &reqwest::Client, uname: &str, pass: &str) -> Result<(), Box<dyn Error>>{
    let mut form_data = HashMap::new();
    form_data.insert("uname", uname);
    form_data.insert("pw", pass);
    form_data.insert("dologin", "log+in");

    client
        .post("https://codingbat.com/login")
        .form(&form_data)
        .send()
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Setup the cli
    let matches = Command::new("batgpt")
        .version("0.0.1")
        .author("Yohan")
        .about("Solves codingbat problems using openai gpt-3")
        .subcommand_required(true)
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enable verbose output"),
        )
        .subcommand(
            Command::new("add")
                .about("Add a student")
                .arg(
                    Arg::new("cuname")
                        .help("Sets the cuname of the student")
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("pass")
                        .help("Sets the password of the student")
                        .required(true)
                        .index(2),
                ),
        )
        .subcommand(
            Command::new("remove").about("Remove a student").arg(
                Arg::new("cuname")
                    .help("Cuname of the student to remove")
                    .required(true)
                    .index(1),
            ),
        )
        .subcommand(Command::new("list").about("List all students"))
        .subcommand(
            Command::new("setkey").about("Set the openai api key").arg(
                Arg::new("key")
                    .help("Sets the openai api key")
                    .required(true)
                    .index(1),
            ),
        )
        .subcommand(
            Command::new("solve").about("Solve a problem").arg(
                Arg::new("prob")
                    .help("Sets the problem to solve")
                    .num_args(1..)
                    .required(true)
                    .index(1),
            ),
        )
        .get_matches();

    // Load the config
    let config: MyConfig = confy::load("batgpt", None)?;

    match matches.subcommand() {
        Some(("add", add_matches)) => {
            let mut students = config.students;
            students.push(Student {
                cuname: (add_matches.get_one::<String>("cuname").unwrap()).clone(),
                pass: (add_matches.get_one::<String>("pass").unwrap()).clone(),
            });

            confy::store(
                "batgpt",
                None,
                &MyConfig {
                    openai_key: config.openai_key,
                    students,
                },
            )?;
        }
        Some(("remove", remove_matches)) => {
            let cuname = (remove_matches.get_one::<String>("cuname").unwrap()).clone();
            let mut students = config.students;
            students.retain(|student| student.cuname != cuname);
            confy::store(
                "batgpt",
                None,
                &MyConfig {
                    openai_key: config.openai_key,
                    students,
                },
            )?;
        }
        Some(("list", _)) => {
            for student in config.students {
                println!("{}: {}", student.cuname, student.pass);
            }
        }
        Some(("setkey", setkey_matches)) => {
            confy::store(
                "batgpt",
                None,
                &MyConfig {
                    openai_key: (setkey_matches.get_one::<String>("key").unwrap()).clone(),
                    students: config.students,
                },
            )?;
        }
        Some(("solve", solve_matches)) => {
            let mut handles = vec![];

            // Set the openai api key
            set_key(config.openai_key);

            // Create the parsing client
            let parse_client = Client::new();

            // Solve all the problems and store their solutions
            let problems = solve_matches.get_many::<String>("prob").unwrap();
            let mut solutions: HashMap<String, String> = HashMap::new();
            for prob in problems {
                let solution = solve_prob(&parse_client, &prob).await;
                solutions.insert(prob.clone(), solution);
            }

            let shared_solutions = Arc::new(solutions);

            // Run the solution for each student
            for student in config.students {
                // Create the client
                let client = ClientBuilder::new()
                    .cookie_store(true)
                    .build()
                    .expect("Failed to create client");

                // Login
                login(&client, &student.cuname, &student.pass).await?;

                // Run the solutions asyncronously
                let solutions = shared_solutions.as_ref().clone();
                for (prob, solution) in solutions {
                    let client = client.clone();
                    let cuname = student.cuname.clone();
                    handles.push(tokio::spawn(async move {
                        run_code(&client, prob.as_str(), solution.as_str(), &cuname).await;
                    }));
                }
            }

            // Wait for all the tasks to finish
            for handle in handles {
                handle.await?;
            }
        }
        _ => {}
    }
    Ok(())
}
