package rustdeps

import (
	"fmt"
	"sort"

	"github.com/pelletier/go-toml/v2"
)

// cargoLockFile represents the structure of a Cargo.lock file.
type cargoLockFile struct {
	Version  int              `toml:"version"`
	Packages []cargoLockEntry `toml:"package"`
}

// cargoLockEntry represents a single package entry in Cargo.lock.
type cargoLockEntry struct {
	Name         string   `toml:"name"`
	Version      string   `toml:"version"`
	Source       string   `toml:"source"`
	Checksum     string   `toml:"checksum"`
	Dependencies []string `toml:"dependencies"`
}

// ParseCargoLock parses Cargo.lock content and extracts crate dependencies.
// It takes raw bytes for testability (no file I/O).
func ParseCargoLock(data []byte, opts ParseOptions) ([]Crate, error) {
	var lockFile cargoLockFile
	if err := toml.Unmarshal(data, &lockFile); err != nil {
		return nil, fmt.Errorf("parsing Cargo.lock: %w", err)
	}

	var crates []Crate
	for _, pkg := range lockFile.Packages {
		// Only include crates from crates.io registry
		if !isCratesIOSource(pkg.Source) {
			continue
		}

		crates = append(crates, Crate{
			Name:     pkg.Name,
			Version:  pkg.Version,
			Source:   pkg.Source,
			Checksum: pkg.Checksum,
		})
	}

	// Sort by crate name for consistent output
	sort.Slice(crates, func(i, j int) bool {
		if crates[i].Name == crates[j].Name {
			return crates[i].Version < crates[j].Version
		}
		return crates[i].Name < crates[j].Name
	})

	return crates, nil
}

// isCratesIOSource returns true if the source is crates.io registry.
func isCratesIOSource(source string) bool {
	return source == "registry+https://github.com/rust-lang/crates.io-index"
}

// ConvertChecksumToSRI converts a hex-encoded SHA256 checksum to SRI format.
// The checksum from Cargo.lock is hex-encoded; Nix wants base64 SRI format.
func ConvertChecksumToSRI(hexChecksum string) (string, error) {
	if hexChecksum == "" {
		return "", nil
	}

	// Parse hex string to bytes
	if len(hexChecksum) != 64 {
		return "", fmt.Errorf("invalid checksum length: expected 64 hex chars, got %d", len(hexChecksum))
	}

	bytes := make([]byte, 32)
	for i := 0; i < 32; i++ {
		b, err := hexByte(hexChecksum[i*2 : i*2+2])
		if err != nil {
			return "", fmt.Errorf("invalid hex in checksum: %w", err)
		}
		bytes[i] = b
	}

	// Encode to base64 for SRI format
	return "sha256-" + base64Encode(bytes), nil
}

// hexByte converts a 2-character hex string to a byte.
func hexByte(s string) (byte, error) {
	if len(s) != 2 {
		return 0, fmt.Errorf("expected 2 hex chars")
	}
	var b byte
	for _, c := range s {
		b <<= 4
		switch {
		case c >= '0' && c <= '9':
			b |= byte(c - '0')
		case c >= 'a' && c <= 'f':
			b |= byte(c - 'a' + 10)
		case c >= 'A' && c <= 'F':
			b |= byte(c - 'A' + 10)
		default:
			return 0, fmt.Errorf("invalid hex char: %c", c)
		}
	}
	return b, nil
}

// base64Encode encodes bytes to base64 (standard encoding).
func base64Encode(data []byte) string {
	const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
	var result []byte

	for i := 0; i < len(data); i += 3 {
		var chunk uint32
		var padding int

		chunk = uint32(data[i]) << 16
		if i+1 < len(data) {
			chunk |= uint32(data[i+1]) << 8
		} else {
			padding++
		}
		if i+2 < len(data) {
			chunk |= uint32(data[i+2])
		} else {
			padding++
		}

		result = append(result, alphabet[(chunk>>18)&0x3F])
		result = append(result, alphabet[(chunk>>12)&0x3F])
		if padding < 2 {
			result = append(result, alphabet[(chunk>>6)&0x3F])
		} else {
			result = append(result, '=')
		}
		if padding < 1 {
			result = append(result, alphabet[chunk&0x3F])
		} else {
			result = append(result, '=')
		}
	}

	return string(result)
}

// PopulateNixHashes converts Cargo.lock checksums to Nix SRI hashes.
func PopulateNixHashes(crates []Crate) error {
	for i := range crates {
		if crates[i].Checksum == "" {
			continue
		}
		sriHash, err := ConvertChecksumToSRI(crates[i].Checksum)
		if err != nil {
			return fmt.Errorf("converting checksum for %s: %w", crates[i].Name, err)
		}
		crates[i].NixHash = sriHash
	}
	return nil
}
