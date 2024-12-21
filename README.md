# Yet Another Downloader

![logo](./src-tauri/icons/white-bg.png)

---

Yet Another Downloader is a download manager. The downloaded files are organised by type inside a
directory called `Yad` inside your default download directory.

In order to download a file, paste it's url in the text box and the download will automatically
start.

**This is a beta version, please report bugs as issues: **

This software is released under the [Apache V2 license](https://choosealicense.com/licenses/apache-2.0/). Please read both the [License](./LICENSE) and [Notice](./NOTICE)




## Installation

This project is built with [Rust](https://www.rust-lang.org/) and [Tauri](https://tauri.app/), It also requires [Node](https://nodejs.org) to be installed. Therefore make sure they're installed firts.

```sh
# install rust 
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# install tauri-cli
cargo install tauri-cli 

# build

cargo tauri build --no-bundle --config src-tauri/tauri.conf.json
```

## Reporting issues

This is still in beta version. Therefore, if you find any issues, please raise it in the [Issues](https://github.com/innv8/yad/issues) tab.

## Contributing

Contribution is welcome. Be it documentation, improving the code, adding tests, anything to help. If
you're interested, please follow the following steps:

1. Identify/ raise an issue 
2. Fork the repository (from the main branch) and clone it.
3. Checkout to your branch with the issue number as part of the branch name. In the example below,
   it's related to issue #12
    ```sh
    git checkout -b fx/issue-12
    ```
4. Make your changes and push them to GitHub 
5. Open a pull request to this repo.


---

You can read about how I made this and more at [innv8.ke](https://innv8.ke). This is my educational
Github Organisation. Please also check my [personal GitHub](https://github.com/rapando)


> rapando
