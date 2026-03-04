Name:           ownstack-ide
Version:        0.4.6.{{{ git_dir_version }}}
Release:        1
Summary:        Rust-native IDE with embedded AI agents
License:        Apache-2.0 AND MIT
URL:            https://github.com/psykoniz/Ownstack

VCS:            {{{ git_dir_vcs }}}
Source:        	{{{ git_dir_pack }}}

BuildRequires:  cargo libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf libxkbcommon-x11-devel

%description
OwnStack IDE is a Rust-native code editor based on Lapce with secure,
integrated AI agent workflows. Built with Floem UI and wgpu rendering.

%prep
{{{ git_dir_setup_macro }}}
cargo fetch --locked

%build
cargo build --profile release-lto --package lapce-app --frozen

%install
install -Dm755 target/release-lto/ownstack-ide %{buildroot}%{_bindir}/ownstack-ide
install -Dm644 extra/linux/io.ownstack.ownstackide.desktop %{buildroot}/usr/share/applications/io.ownstack.ownstackide.desktop
install -Dm644 extra/linux/io.ownstack.ownstackide.metainfo.xml %{buildroot}/usr/share/metainfo/io.ownstack.ownstackide.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/io.ownstack.ownstackide.png

%files
%license LICENSE* NOTICE
%doc *.md
%{_bindir}/ownstack-ide
/usr/share/applications/io.ownstack.ownstackide.desktop
/usr/share/metainfo/io.ownstack.ownstackide.metainfo.xml
/usr/share/pixmaps/io.ownstack.ownstackide.png

%changelog
* Tue Mar 04 2026 OwnStack Contributors
- Rebrand from Lapce to OwnStack IDE
- See full changelog on GitHub
