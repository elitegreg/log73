# Radio Client verification matrix

The automated Radio Client verification entry point is:

```bash
make verify-radio-client
```

It runs repository CI, builds all debug binaries, validates native package
staging contents, exercises the dummy loopback host, and uses a local Axum HTTP
fixture to verify registration, heartbeat, and offline lease requests. If
`cargo-dist` is installed, it also checks that the release manifest names
`log73-radio-client`.

The following checks require hardware or target operating systems and are
explicitly deferred from Linux CI:

- CAT operation against each supported physical radio and serial transport.
- Winkeyer, serial DTR/RTS, and local audio input/output devices.
- WSJT-X UDP interoperability with a running WSJT-X process.
- Windows MSI installation/desktop launch and macOS package launch.
- Native DEB/RPM installation and desktop launch on each supported Linux
  architecture.

Before publishing a release, run the automated command and then execute the
deferred matrix on the target systems with a dummy radio first, followed by
the station's configured hardware.
