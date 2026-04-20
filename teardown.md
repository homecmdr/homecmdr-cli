# HomeCmdr Teardown

## 1. Stop and remove the service

```bash
homecmdr service uninstall
```

## 2. Remove system directories, binary, and user

```bash
sudo rm -rf /etc/homecmdr /var/lib/homecmdr
sudo rm -f /usr/local/bin/homecmdr
sudo userdel homecmdr
```

## 3. Remove the workspace and state file

```bash
rm -rf ~/.local/share/homecmdr
rm -rf ~/.config/homecmdr
```

## 4. Uninstall the CLI binary

```bash
cargo uninstall homecmdr-cli
```

---

## Verify everything is gone

```bash
which homecmdr                          # should print nothing
ls ~/.local/share/homecmdr 2>&1         # should say no such file
ls ~/.config/homecmdr 2>&1              # should say no such file
sudo ls /etc/homecmdr 2>&1              # should say no such file
id homecmdr 2>&1                        # should say no such user
```

## Fresh install flow

```bash
cargo install --git https://github.com/homecmdr/homecmdr-cli
homecmdr init
```