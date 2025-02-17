use mrial_fs::{storage::StorageMultiType, Users, User};

fn handle_user_add_cli(args: &[String], users: &mut Users) {
    if args.len() == 0 || args.len() != 2 {
        println!("
\"mrial_server user add\" requires 2 arguments.
        
Usage \"mrial_server user add [username] [password]\"

For more help on how to use Mrial CLI, head to https://github.com/mahitmehta/mrial\"\n");
        return;
    }

    let username = &args[0];
    let pass = &args[1];

    if let Some(_) = users.find(username.to_string()) {
        println!("User already exists.");
        return;
    }

    let new_user = User {
        username: username.clone(),
        pass: pass.clone(),
    };

    if let Err(e) = users.add(new_user) {
        println!("Error adding user: {}", e);
        return;
    }

    if let Err(e) = users.save() {
        println!("Error saving users: {}", e);
        return;
    }

    println!("User added successfully.");
}

fn handle_user_rm_cli(args: &[String], users: &mut Users) {
    if args.len() == 0 {
        println!("
\"mrial_server user rm\" requires 1 argument.

Usage \"mrial_server user rm [username]\"

For more help on how to use Mrial CLI, head to https://github.com/mahitmehta/mrial\n");
        return;
    }

    let username = &args[0];

    if let None = users.find(username.to_string()) {
        println!("User not found.");
        return;
    }

    if let Err(e) = users.remove(username.to_string()) {
        println!("Error removing user: {}", e);
        return;
    }

    if let Err(e) = users.save() {
        println!("Error saving users: {}", e);
        return;
    }

    println!("User removed successfully.");
}

fn handle_user_cli(args: &[String]) {
    if args.len() == 0 {
        print_user_help();
        return;
    }

    let cmd = &args[0];

    let mut users = Users::new();
    if let Err(e) = users.load() {
        println!("Error loading users: {}", e);
        return;
    }

    if cmd == "ls" {
        println!("Authenticated Users:");

        if let Some(users) = &users.users.get() {
            for i in 0..users.len() {
                println!("{}. {}", (i + 1), users[i].username);
            } 
            if users.len() == 0 {
                println!("No users found.");
            }
        } else {
            println!("Failed to get users.");
        }
    } else if cmd == "add" {
        handle_user_add_cli(&args[1..], &mut users);  
    } else if cmd == "rm" {
        handle_user_rm_cli(&args[1..], &mut users);  
    } else if cmd == "--help" {
        print_user_help();
    } else {
        println!("Invalid Option.\n\nUse `--help` for more information.");
    }
}

fn print_user_help() {
    println!("
Usage: mrial_server user [options]

Commands:\n
    ls\t\tList authenticated users
    add\t\tAdd a new user
    rm\t\tRemove a user

Flags:\n
    --help\t\tShow this help message
\nFor more help on how to use Mrial CLI, head to https://github.com/mahitmehta/mrial\n");
}
    
fn print_help() {
    println!("
Usage: mrial_server [command] [options]

Commands:\n
    user\t\tManage authenticated users

Flags:\n
    --help\t\tShow this help message
\nFor more help on how to use Mrial CLI, head to https://github.com/mahitmehta/mrial");
}

pub fn handle_cli(args: &Vec<String>) {
    let cmd = &args[1];

    if cmd == "user" {
        let user_args = &args[2..];
        handle_user_cli(user_args);
    } else if cmd == "--help" {
        print_help();
    } else {
        println!("Invalid Option.\nUse `--help` for more information.");
    }
}