# haraltr

Proximity-based authentication for Linux. A daemon monitors the RSSI of connected Bluetooth devices and computes Relative Path Loss (RPL). when a trusted device leaves your vicinity, your session locks via systemd-logind and when it returns the session unlocks.

A bundled PAM module lets PAM users authenticate the same way. Configuration and live status are exposed through a small web UI.

## Status

Early. Only one desktop environment - KDE Plasma - has been confirmed to work with the logind unlock D-Bus call. Other DEs may lock fine but ignore the unlock signal. In that case PAM module can be used for simplified unlock.

BLE support (especially 5.2+) is limited.

## First run

```
sudo haraltr passwd                          # seed the UI password
sudo systemctl enable --now haraltr.service
xdg-open http://127.0.0.1:15999              # open UI config
```

Or use `/etc/haraltr/config.toml` and no UI.

## PAM integration

The package comes with a PAM module. Place PAM rule with `pam_haraltr.so` anywhere in `/etc/pam.d/`. Base this decision on your security needs. Section for recommended rules is coming.

## Configuration

Lives at `/etc/haraltr/config.toml`. The web UI is the easiest way to edit it.
See `config.example.toml` for the full schema.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
