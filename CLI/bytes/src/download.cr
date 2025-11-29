require "http/client"
require "colorize"
require "uri"
def download_with_progress(url : String, dest : String) : Nil
  headers = HTTP::Headers{"User-Agent" => "bytes-io-cli/0.5 (Crystal)"}
  max_redirects = 10
  current_url = url
  file_name = File.basename(url)
  while max_redirects > 0
    parsed_url = URI.parse(current_url)
    client = HTTP::Client.new(parsed_url)
    client.get(parsed_url.path || "/", headers: headers) do |response|
      if response.status_code == 200
        total = response.headers["Content-Length"]?.try(&.to_i64) || 0_i64
        read = 0_i64
        File.open(dest, "wb") do |file|
          buffer = Bytes.new(4096)
          while (bytes_read = response.body_io.read(buffer)) > 0
            file.write(buffer[0...bytes_read])
            read += bytes_read
            if total > 0
              percent = (read.to_f / total * 100).round(2)
              print "\r#{info_style("Downloading #{file_name}...")} #{percent}% of #{total} bytes".ljust(80)
            else
              print "\r#{info_style("Downloading #{file_name}...")} #{read} bytes downloaded".ljust(80)
            end
            STDOUT.flush
          end
        end
        puts # New line after progress
        return
      elsif response.status_code.in?(301, 302, 303, 307, 308)
        location = response.headers["Location"]?
        if location
          current_url = location.starts_with?("http") ? location : parsed_url.resolve(location).to_s
          max_redirects -= 1
        else
          raise "Redirect without location"
        end
      else
        raise "Bad status: #{response.status_code}"
      end
    end
  end
  raise "Too many redirects"
end

