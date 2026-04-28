# Home-manager module for turnkey-composed
#
# Manages the FUSE composition daemon as a user service.
# Generates the config file and service definition from declarative Nix options.
#
# Usage in home-manager config:
#
#   imports = [ turnkey.homeManagerModules.turnkey-composed ];
#
#   services.turnkey-composed = {
#     enable = true;
#     mounts = {
#       turnkey = {
#         repo = "/Users/yann/src/github.com/firefly-engineering/turnkey";
#         mountPoint = "/firefly/turnkey";
#       };
#       other-project = {
#         repo = "/Users/yann/src/other-project";
#         mountPoint = "/firefly/other";
#         backend = "fuse";
#       };
#     };
#   };

{ config, lib, pkgs, ... }:

let
  cfg = config.services.turnkey-composed;

  mountType = lib.types.submodule {
    options = {
      repo = lib.mkOption {
        type = lib.types.str;
        description = "Absolute path to the repository root (must contain a flake.nix)";
      };

      mountPoint = lib.mkOption {
        type = lib.types.str;
        description = "Where to mount the composed view (e.g., /firefly/myproject)";
      };

      backend = lib.mkOption {
        type = lib.types.enum [ "auto" "fuse" "symlink" ];
        default = "auto";
        description = "Backend type: auto, fuse, or symlink";
      };
    };
  };

  # Generate the composed.toml content
  configContent = lib.concatStringsSep "\n" (
    lib.mapAttrsToList (_name: mount: ''
      [[mounts]]
      repo = "${mount.repo}"
      mount_point = "${mount.mountPoint}"
      backend = "${mount.backend}"
    '') cfg.mounts
  );

  configFile = pkgs.writeText "turnkey-composed.toml" configContent;

in
{
  options.services.turnkey-composed = {
    enable = lib.mkEnableOption "Turnkey FUSE composition daemon";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The turnkey-composed package to use";
    };

    mounts = lib.mkOption {
      type = lib.types.attrsOf mountType;
      default = { };
      description = ''
        Mount entries for the composition daemon.
        Each entry maps a repository to a mount point.
      '';
      example = lib.literalExpression ''
        {
          myproject = {
            repo = "/home/user/src/myproject";
            mountPoint = "/firefly/myproject";
          };
        }
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # Place the config file
    xdg.configFile."turnkey/composed.toml".text = configContent;

    # macOS: launchd agent
    launchd.agents.turnkey-composed = lib.mkIf pkgs.stdenv.isDarwin {
      enable = true;
      config = {
        Label = "com.firefly.turnkey-composed";
        ProgramArguments = [
          "${cfg.package}/bin/turnkey-composed"
          "serve"
          "--config"
          "${config.xdg.configHome}/turnkey/composed.toml"
        ];
        RunAtLoad = true;
        KeepAlive = {
          SuccessfulExit = false;
        };
        StandardOutPath = "/tmp/turnkey-composed.stdout.log";
        StandardErrorPath = "/tmp/turnkey-composed.stderr.log";
        EnvironmentVariables = {
          PATH = "/usr/local/bin:/usr/bin:/bin:/nix/var/nix/profiles/default/bin:${config.home.profileDirectory}/bin";
          RUST_LOG = "info";
        };
      };
    };

    # Linux: systemd user service
    systemd.user.services.turnkey-composed = lib.mkIf pkgs.stdenv.isLinux {
      Unit = {
        Description = "Turnkey FUSE Composition Daemon";
        After = [ "nix-daemon.service" ];
      };
      Service = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/turnkey-composed serve --config ${config.xdg.configHome}/turnkey/composed.toml";
        Restart = "on-failure";
        RestartSec = 5;
        Environment = [
          "RUST_LOG=info"
          "PATH=/usr/local/bin:/usr/bin:/bin:${config.home.profileDirectory}/bin"
        ];
      };
      Install = {
        WantedBy = [ "default.target" ];
      };
    };
  };
}
