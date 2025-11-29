require "http/client"
require "yaml"
require "file_utils"
alias Repo = Hash(String, Hash(String, Hash(String, String)))
def refresh_repo(url : String, local_path : String) : Nil
  response = HTTP::Client.get(url)
  if response.status_code != 200
    raise "Bad status: #{response.status_code}"
  end
  Dir.mkdir_p(File.dirname(local_path))
  File.write(local_path, response.body)
end
def parse_repo(path : String) : Repo
  data = File.read(path)
  raw = YAML.parse(data).as_h
  repo = Repo.new
  raw.each do |section, sec_val|
    next unless sec_val.as_h?
    sec_map = sec_val.as_h
    section_str = section.as_s? || next
    repo[section_str] = Hash(String, Hash(String, String)).new
    sec_map.each do |category, cat_val|
      cat_str = category.as_s? || next
      if cat_val.nil?
        repo[section_str][cat_str] = Hash(String, String).new
        next
      end
      next unless cat_val.as_h?
      cat_map = cat_val.as_h
      repo[section_str][cat_str] = Hash(String, String).new
      cat_map.each do |name, url_val|
        name_str = name.as_s? || next
        url = url_val.try(&.as_s?) || ""
        repo[section_str][cat_str][name_str] = url
      end
    end
  end
  repo
end

