Name:           audiodub
Version:        0.4.8
Release:        1%{?dist}
Summary:        AI-powered spoken audio translation and dubbing for Linux

License:        MIT
URL:            https://github.com/digitalninjanv/dubing
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.75
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4) >= 4.12
BuildRequires:  pkgconfig(libadwaita-1) >= 1.4
BuildRequires:  openssl-devel

Requires:       gtk4 >= 4.12
Requires:       libadwaita >= 1.4
Requires:       ffmpeg

%description
AudioDub AI is a native Linux desktop application built with GTK4 and Libadwaita
that converts spoken audio files into translated voice tracks. It provides
automatic language identification, speaker diarization, natural timestamp-aligned
voice generation using Google Gemini, and deterministic local audio assembly via FFmpeg.

%prep
%autosetup

%build
cargo build --release --locked

%install
rm -rf $RPM_BUILD_ROOT
install -D -p -m 0755 target/release/audiodub %{buildroot}%{_bindir}/audiodub
install -D -p -m 0644 data/io.github.digitalninjanv.AudioDub.desktop %{buildroot}%{_datadir}/applications/io.github.digitalninjanv.AudioDub.desktop
install -D -p -m 0644 data/io.github.digitalninjanv.AudioDub.metainfo.xml %{buildroot}%{_datadir}/metainfo/io.github.digitalninjanv.AudioDub.metainfo.xml
install -D -p -m 0644 data/icons/hicolor/scalable/apps/io.github.digitalninjanv.AudioDub.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.digitalninjanv.AudioDub.svg

%check
cargo test --release

%files
%license LICENSE
%doc README.md
%{_bindir}/audiodub
%{_datadir}/applications/io.github.digitalninjanv.AudioDub.desktop
%{_datadir}/metainfo/io.github.digitalninjanv.AudioDub.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.digitalninjanv.AudioDub.svg

%changelog
* Sun Sep 06 2026 AudioDub Contributors <info@audiodub.local> - 0.1.0-1
- Initial Fedora 44 release
