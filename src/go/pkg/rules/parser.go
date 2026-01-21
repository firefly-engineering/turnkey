package rules

import (
	"bufio"
	"fmt"
	"os"
	"regexp"
	"strings"
)

// Parser parses and generates rules.star files.
type Parser struct {
	// Indent is the indentation string to use (default: 4 spaces).
	Indent string
}

// NewParser creates a new Parser with default settings.
func NewParser() *Parser {
	return &Parser{
		Indent: "    ",
	}
}

// Regular expressions for parsing rules.star files.
var (
	// Matches: load("@prelude//:rules.bzl", "go_binary", "go_test")
	loadRe = regexp.MustCompile(`^load\(`)

	// Matches: go_binary( or rust_library( etc.
	targetStartRe = regexp.MustCompile(`^(\w+)\($`)

	// Matches: name = "target-name",
	nameRe = regexp.MustCompile(`^\s*name\s*=\s*"([^"]+)"`)

	// Matches: srcs = ["file.go", ...] or srcs = glob(["**/*.go"])
	srcsStartRe = regexp.MustCompile(`^\s*srcs\s*=\s*`)

	// Matches: deps = [
	depsStartRe = regexp.MustCompile(`^\s*deps\s*=\s*\[`)

	// Matches: "some/path:target",
	depEntryRe = regexp.MustCompile(`^\s*"([^"]+)"`)

	// Matches: # Auto-managed by turnkey. Hash: abc123
	headerHashRe = regexp.MustCompile(`^#\s*Auto-managed by turnkey\.\s*Hash:\s*(\w+)`)

	// Matches: visibility = ["PUBLIC"],
	visibilityRe = regexp.MustCompile(`^\s*visibility\s*=`)
)

// ParseFile reads and parses a rules.star file.
func (p *Parser) ParseFile(path string) (*RulesFile, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read %s: %w", path, err)
	}

	return p.Parse(path, string(content))
}

// Parse parses rules.star content.
func (p *Parser) Parse(path, content string) (*RulesFile, error) {
	rf := &RulesFile{
		Path:       path,
		RawContent: content,
	}

	scanner := bufio.NewScanner(strings.NewReader(content))
	lineNum := 0
	var currentTarget *Target
	inDeps := false
	inAutoSection := false
	inPreserveSection := false
	braceDepth := 0

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		// Check for header hash
		if matches := headerHashRe.FindStringSubmatch(trimmed); len(matches) > 1 {
			rf.Hash = matches[1]
			continue
		}

		// Check for load statements
		if loadRe.MatchString(trimmed) {
			rf.Loads = append(rf.Loads, line)
			continue
		}

		// Check for target start (rule_name()
		if matches := targetStartRe.FindStringSubmatch(trimmed); len(matches) > 1 {
			currentTarget = &Target{
				Rule:      matches[1],
				StartLine: lineNum,
			}
			braceDepth = 1
			continue
		}

		// If we're inside a target
		if currentTarget != nil {
			// Track brace depth
			braceDepth += strings.Count(line, "(") - strings.Count(line, ")")

			// Check for target end
			if braceDepth <= 0 {
				currentTarget.EndLine = lineNum
				rf.Targets = append(rf.Targets, currentTarget)
				currentTarget = nil
				inDeps = false
				inAutoSection = false
				inPreserveSection = false
				continue
			}

			// Check for name attribute
			if matches := nameRe.FindStringSubmatch(line); len(matches) > 1 {
				currentTarget.Name = matches[1]
				continue
			}

			// Check for deps start
			if depsStartRe.MatchString(line) {
				inDeps = true
				// Check if deps are on same line: deps = ["foo"],
				if strings.Contains(line, "]") {
					inDeps = false
					// Extract inline deps
					p.extractInlineDeps(line, currentTarget)
				}
				continue
			}

			// If we're in deps section
			if inDeps {
				// Check for section markers
				if strings.Contains(trimmed, MarkerAutoStart) {
					inAutoSection = true
					continue
				}
				if strings.Contains(trimmed, MarkerAutoEnd) {
					inAutoSection = false
					continue
				}
				if strings.Contains(trimmed, MarkerPreserveStart) {
					inPreserveSection = true
					continue
				}
				if strings.Contains(trimmed, MarkerPreserveEnd) {
					inPreserveSection = false
					continue
				}

				// Check for deps section end
				if strings.Contains(trimmed, "]") {
					inDeps = false
					// Extract any dep on this line before ]
					p.extractDepFromLine(trimmed, currentTarget, inAutoSection, inPreserveSection)
					continue
				}

				// Extract dependency entry
				p.extractDepFromLine(trimmed, currentTarget, inAutoSection, inPreserveSection)
			}
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("failed to scan %s: %w", path, err)
	}

	return rf, nil
}

// extractInlineDeps extracts deps from a single-line deps declaration.
func (p *Parser) extractInlineDeps(line string, target *Target) {
	// Find content between [ and ]
	start := strings.Index(line, "[")
	end := strings.LastIndex(line, "]")
	if start < 0 || end < 0 || end <= start {
		return
	}

	content := line[start+1 : end]
	// Split by comma and extract strings
	for _, part := range strings.Split(content, ",") {
		part = strings.TrimSpace(part)
		if matches := depEntryRe.FindStringSubmatch(part); len(matches) > 1 {
			dep := matches[1]
			target.Deps = append(target.Deps, dep)
			// Without markers, all deps are considered auto-managed
			target.AutoDeps = append(target.AutoDeps, dep)
		}
	}
}

// extractDepFromLine extracts a dependency from a line.
func (p *Parser) extractDepFromLine(line string, target *Target, inAuto, inPreserve bool) {
	if matches := depEntryRe.FindStringSubmatch(line); len(matches) > 1 {
		dep := matches[1]
		target.Deps = append(target.Deps, dep)

		if inPreserve {
			target.PreservedDeps = append(target.PreservedDeps, dep)
		} else {
			// If in auto section or no markers, treat as auto-managed
			target.AutoDeps = append(target.AutoDeps, dep)
		}
	}
}

// GenerateTarget generates the rules.star content for a target with updated deps.
func (p *Parser) GenerateTarget(target *Target, newAutoDeps []string) string {
	indent := p.Indent

	var lines []string
	lines = append(lines, fmt.Sprintf("%s(", target.Rule))
	lines = append(lines, fmt.Sprintf("%sname = \"%s\",", indent, target.Name))

	// Add srcs if present
	if len(target.Srcs) > 0 {
		if len(target.Srcs) == 1 {
			lines = append(lines, fmt.Sprintf("%ssrcs = [\"%s\"],", indent, target.Srcs[0]))
		} else {
			lines = append(lines, fmt.Sprintf("%ssrcs = [", indent))
			for _, src := range target.Srcs {
				lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, src))
			}
			lines = append(lines, fmt.Sprintf("%s],", indent))
		}
	}

	// Add deps with markers
	allDeps := append(newAutoDeps, target.PreservedDeps...)
	if len(allDeps) > 0 {
		lines = append(lines, fmt.Sprintf("%sdeps = [", indent))
		lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerAutoStart))
		for _, dep := range newAutoDeps {
			lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, dep))
		}
		lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerAutoEnd))

		if len(target.PreservedDeps) > 0 {
			lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerPreserveStart))
			for _, dep := range target.PreservedDeps {
				lines = append(lines, fmt.Sprintf("%s%s\"%s\",", indent, indent, dep))
			}
			lines = append(lines, fmt.Sprintf("%s%s%s", indent, indent, MarkerPreserveEnd))
		}
		lines = append(lines, fmt.Sprintf("%s],", indent))
	}

	lines = append(lines, fmt.Sprintf("%svisibility = [\"PUBLIC\"],", indent))
	lines = append(lines, ")")

	return strings.Join(lines, "\n")
}

// GenerateHeader generates the file header with hash comment.
func (p *Parser) GenerateHeader(hash string) string {
	return fmt.Sprintf("# Auto-managed by turnkey. Hash: %s\n# Manual sections marked with turnkey:preserve-start/end are not modified.\n", hash)
}

// FindTarget finds a target by name in a rules file.
func (rf *RulesFile) FindTarget(name string) *Target {
	for _, t := range rf.Targets {
		if t.Name == name {
			return t
		}
	}
	return nil
}

// HasMarkers returns true if any target has turnkey markers.
func (rf *RulesFile) HasMarkers() bool {
	return strings.Contains(rf.RawContent, MarkerAutoStart) ||
		strings.Contains(rf.RawContent, MarkerPreserveStart)
}
