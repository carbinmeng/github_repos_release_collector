# github_repos_release_collector
a  tiny tool written in rust to collector release info from github of repos in the custom list

## Installation

### Prerequisites

#### Debian/Ubuntu Systems
Before building the project, you need to install the following dependencies:

```bash
sudo apt-get update
sudo apt-get install -y pkg-config libssl-dev build-essential
```

These packages provide the necessary OpenSSL development libraries and build tools required by the project dependencies.

#### Other Linux Systems
Ensure you have the equivalent packages for your distribution:
- `pkg-config` or similar tool
- OpenSSL development libraries (often named `openssl-devel` or `libssl-dev`)
- Basic build tools (gcc, make, etc.)

### Building the Project

Once dependencies are installed, you can build the project using Cargo:

```bash
# Build in debug mode
cargo build

# Build in release mode (optimized)
cargo build --release
```

