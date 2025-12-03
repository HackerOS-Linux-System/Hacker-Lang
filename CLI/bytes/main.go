package main

import (
	"bufio"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

var baseDir string

func init() {
	home, err := os.UserHomeDir()
	if err != nil {
		panic(err)
	}
	baseDir = filepath.Join(home, ".hackeros", "hacker-lang")
	os.MkdirAll(filepath.Join(baseDir, "libs"), 0755)
	os.MkdirAll(filepath.Join(baseDir, "plugins"), 0755)
	os.MkdirAll(filepath.Join(baseDir, "sources"), 0755)
}

const (
	libRepoURL    = "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/bytes.io"
	pluginRepoURL = "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/plugins-repo.hacker"
	sourceRepoURL = "https://raw.githubusercontent.com/Bytes-Repository/bytes.io/main/repository/source-repo.hacker"
)

func getRepoContent(url string) (string, error) {
	resp, err := http.Get(url)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

func parseRepo(content string) map[string]string {
	repo := make(map[string]string)
	scanner := bufio.NewScanner(strings.NewReader(content))
	var section string
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		if strings.HasSuffix(line, ":") {
			section = strings.TrimSpace(line[:len(line)-1])
			continue
		}
		if strings.Contains(line, ":") && section != "" {
			parts := strings.SplitN(line, ":", 2)
			key := strings.Trim(parts[0], " \t\"")
			value := strings.Trim(parts[1], " \t\"")
			repo[key] = value
		}
	}
	return repo
}

func downloadFile(url, path string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	_, err = io.Copy(f, resp.Body)
	return err
}

var installCmd = &cobra.Command{
	Use:   "install [library]",
	Short: "Install a library",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		content, err := getRepoContent(libRepoURL)
		if err != nil {
			return err
		}
		repo := parseRepo(content)
		url, ok := repo[name]
		if !ok {
			return fmt.Errorf("library %s not found", name)
		}
		filename := filepath.Base(url)
		path := filepath.Join(baseDir, "libs", filename)
		if err := downloadFile(url, path); err != nil {
			return err
		}
		fmt.Printf("Installed %s to %s\n", name, path)
		return nil
	},
}

var removeCmd = &cobra.Command{
	Use:   "remove [library]",
	Short: "Remove a library",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		content, err := getRepoContent(libRepoURL)
		if err != nil {
			return err
		}
		repo := parseRepo(content)
		url, ok := repo[name]
		if !ok {
			return fmt.Errorf("library %s not found", name)
		}
		filename := filepath.Base(url)
		path := filepath.Join(baseDir, "libs", filename)
		if err := os.Remove(path); err != nil {
			if os.IsNotExist(err) {
				fmt.Printf("%s not installed\n", name)
				return nil
			}
			return err
		}
		fmt.Printf("Removed %s\n", name)
		return nil
	},
}

var pluginCmd = &cobra.Command{
	Use:   "plugin",
	Short: "Manage plugins",
}

var pluginInstallCmd = &cobra.Command{
	Use:   "install [plugin]",
	Short: "Install a plugin",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		content, err := getRepoContent(pluginRepoURL)
		if err != nil {
			return err
		}
		repo := parseRepo(content)
		url, ok := repo[name]
		if !ok {
			return fmt.Errorf("plugin %s not found", name)
		}
		filename := filepath.Base(url)
		path := filepath.Join(baseDir, "plugins", filename)
		if err := downloadFile(url, path); err != nil {
			return err
		}
		fmt.Printf("Installed plugin %s to %s\n", name, path)
		return nil
	},
}

var pluginRemoveCmd = &cobra.Command{
	Use:   "remove [plugin]",
	Short: "Remove a plugin",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		content, err := getRepoContent(pluginRepoURL)
		if err != nil {
			return err
		}
		repo := parseRepo(content)
		url, ok := repo[name]
		if !ok {
			return fmt.Errorf("plugin %s not found", name)
		}
		filename := filepath.Base(url)
		path := filepath.Join(baseDir, "plugins", filename)
		if err := os.Remove(path); err != nil {
			if os.IsNotExist(err) {
				fmt.Printf("%s not installed\n", name)
				return nil
			}
			return err
		}
		fmt.Printf("Removed plugin %s\n", name)
		return nil
	},
}

var sourceCmd = &cobra.Command{
	Use:   "source",
	Short: "Manage sources",
}

var sourceInstallCmd = &cobra.Command{
	Use:   "install [source]",
	Short: "Install a source",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		noBuild, err := cmd.Flags().GetBool("no-build")
		if err != nil {
			return err
		}
		content, err := getRepoContent(sourceRepoURL)
		if err != nil {
			return err
		}
		repo := parseRepo(content)
		url, ok := repo[name]
		if !ok {
			return fmt.Errorf("source %s not found", name)
		}
		path := filepath.Join(baseDir, "sources", name)
		if _, err := os.Stat(path); err == nil {
			return fmt.Errorf("source %s already exists", name)
		}
		cloneCmd := exec.Command("git", "clone", url, path)
		if output, err := cloneCmd.CombinedOutput(); err != nil {
			return fmt.Errorf("git clone failed: %v\n%s", err, output)
		}
		if !noBuild {
			buildFile := filepath.Join(path, "build.hacker")
			if _, err := os.Stat(buildFile); err == nil {
				runCmd := exec.Command("hackerc", "run", buildFile)
				runCmd.Dir = path
				if output, err := runCmd.CombinedOutput(); err != nil {
					return fmt.Errorf("hackerc run failed: %v\n%s", err, output)
				}
			} else {
				fmt.Printf("build.hacker not found, skipping build\n")
			}
		}
		fmt.Printf("Installed source %s to %s\n", name, path)
		return nil
	},
}

var sourceRemoveCmd = &cobra.Command{
	Use:   "remove [source]",
	Short: "Remove a source",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		name := args[0]
		path := filepath.Join(baseDir, "sources", name)
		if err := os.RemoveAll(path); err != nil {
			if os.IsNotExist(err) {
				fmt.Printf("%s not installed\n", name)
				return nil
			}
			return err
		}
		fmt.Printf("Removed source %s\n", name)
		return nil
	},
}

func main() {
	rootCmd := &cobra.Command{
		Use:   "bytes",
		Short: "Bytes Manager CLI for Hacker Lang",
	}
	rootCmd.AddCommand(installCmd)
	rootCmd.AddCommand(removeCmd)
	pluginCmd.AddCommand(pluginInstallCmd)
	pluginCmd.AddCommand(pluginRemoveCmd)
	rootCmd.AddCommand(pluginCmd)
	sourceInstallCmd.Flags().Bool("no-build", false, "Skip running build.hacker after cloning")
	sourceCmd.AddCommand(sourceInstallCmd)
	sourceCmd.AddCommand(sourceRemoveCmd)
	rootCmd.AddCommand(sourceCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}

