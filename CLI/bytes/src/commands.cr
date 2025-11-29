require "file_utils"
require "time"
def search_packages(repo : Repo, query : String)
  found = false
  puts header_style("Search Results for: #{query}")
  repo.each do |section, categories|
    puts bold_style("#{section}:")
    categories.each do |category, packages|
      puts " #{info_style("#{category}:")}" unless category.empty?
      packages.each do |name, url|
        url_display = url.empty? ? "(no release yet)" : url
        if name.downcase.includes?(query.downcase)
          puts " #{success_style(name)} => #{url_display}"
          found = true
        end
      end
    end
  end
  puts warn_style("No packages found matching: #{query}") unless found
end
def install_package(repo : Repo, pkg : String, lib_dir : String) : Bool
  url = ""
  found = false
  repo.each_value do |categories|
    categories.each_value do |packages|
      packages.each do |name, u|
        if name.downcase == pkg.downcase
          url = u
          found = true
          break
        end
      end
      break if found
    end
    break if found
  end
  if !found
    puts error_style("Package not found: #{pkg}")
    return false
  end
  if url.empty?
    puts error_style("No release found for package: #{pkg}")
    return false
  end
  dest = File.join(lib_dir, pkg)
  tmp_dest = File.join(Dir.tempdir, "#{pkg}-#{Time.utc.to_unix}")
  puts info_style("Downloading #{pkg} from #{url}")
  begin
    download_with_progress(url, tmp_dest)
  rescue ex
    puts error_style("Error downloading: #{ex.message}")
    File.delete(tmp_dest) if File.exists?(tmp_dest)
    return false
  end
  # Remove existing
  if File.exists?(dest)
    begin
      File.delete(dest)
    rescue ex
      puts error_style("Error removing old version: #{ex.message}")
      File.delete(tmp_dest) if File.exists?(tmp_dest)
      return false
    end
  end
  # Move (copy + delete for cross-device)
  begin
    FileUtils.cp(tmp_dest, dest)
    File.chmod(dest, 0o755)
    File.delete(tmp_dest)
  rescue ex
    puts error_style("Error installing: #{ex.message}")
    File.delete(tmp_dest) if File.exists?(tmp_dest)
    return false
  end
  puts success_style("Installed #{pkg} to #{lib_dir}")
  true
end
def remove_package(pkg : String, lib_dir : String)
  path = File.join(lib_dir, pkg)
  unless File.exists?(path)
    puts warn_style("Package not installed: #{pkg}")
    return
  end
  begin
    File.delete(path)
  rescue ex
    puts error_style("Error removing: #{ex.message}")
    return
  end
  puts success_style("Removed #{pkg}")
end
def update_packages(lib_dir : String, local_repo : String)
  files = Dir.entries(lib_dir).select { |f| !File.directory?(File.join(lib_dir, f)) && ![".", ".."].includes?(f) }
  repo = begin
    parse_repo(local_repo)
  rescue ex
    puts error_style("Error parsing repo: #{ex.message}")
    return
  end
  updated = 0
  files.each do |file|
    pkg = file
    puts info_style("Checking update for #{pkg}")
    if install_package(repo, pkg, lib_dir)
      updated += 1
    end
  end
  if updated == 0
    puts warn_style("No packages installed to update.")
  else
    puts success_style("Updated #{updated} packages.")
  end
end
