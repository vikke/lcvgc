local M = {}

-- 設定
M.config = {
  host = "127.0.0.1",
  port = 5555,
}

-- TCP接続
local client = nil

function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})
end

-- 接続
function M.connect()
  local uv = vim.uv or vim.loop

  if client then
    client:close()
    client = nil
  end

  client = uv.new_tcp()
  client:connect(M.config.host, M.config.port, function(err)
    if err then
      vim.schedule(function()
        vim.notify("lcvgc: connection failed: " .. err, vim.log.levels.ERROR)
      end)
      client:close()
      client = nil
      return
    end
    vim.schedule(function()
      vim.notify("lcvgc: connected to " .. M.config.host .. ":" .. M.config.port)
    end)
  end)
end

-- 切断
function M.disconnect()
  if client then
    client:close()
    client = nil
    vim.notify("lcvgc: disconnected")
  end
end

-- 送信
function M.send(request, callback)
  if not client then
    vim.notify("lcvgc: not connected. Run :LcvgcConnect first", vim.log.levels.WARN)
    return
  end

  local json = vim.fn.json_encode(request) .. "\n"
  client:write(json)

  -- Read response
  client:read_start(function(err, data)
    client:read_stop()
    if err or not data then return end
    vim.schedule(function()
      local ok, response = pcall(vim.fn.json_decode, data)
      if ok and callback then
        callback(response)
      end
    end)
  end)
end

-- :LcvgcEval - 選択テキストまたは現在行を評価
function M.eval(opts)
  local lines
  if opts and opts.range and opts.range > 0 then
    lines = vim.api.nvim_buf_get_lines(0, opts.line1 - 1, opts.line2, false)
  else
    lines = { vim.api.nvim_get_current_line() }
  end

  local source = table.concat(lines, "\n")
  M.send({ type = "eval", source = source }, function(response)
    if response.success then
      vim.notify("lcvgc: " .. (response.message or "OK"))
    else
      vim.notify("lcvgc: " .. (response.error or "error"), vim.log.levels.ERROR)
    end
  end)
end

-- :LcvgcLoad - ファイル読み込み
function M.load(path)
  M.send({ type = "load", path = path }, function(response)
    if response.success then
      vim.notify("lcvgc: loaded " .. path)
    else
      vim.notify("lcvgc: " .. (response.error or "error"), vim.log.levels.ERROR)
    end
  end)
end

-- :LcvgcPlay - シーン再生
function M.play(scene_name)
  local source = "play " .. scene_name .. " [loop]"
  M.send({ type = "eval", source = source }, function(response)
    if response.success then
      vim.notify("lcvgc: playing " .. scene_name)
    else
      vim.notify("lcvgc: " .. (response.error or "error"), vim.log.levels.ERROR)
    end
  end)
end

-- :LcvgcStop
function M.stop(target)
  local source = target and ("stop " .. target) or "stop"
  M.send({ type = "eval", source = source }, function(response)
    if response.success then
      vim.notify("lcvgc: stopped")
    else
      vim.notify("lcvgc: " .. (response.error or "error"), vim.log.levels.ERROR)
    end
  end)
end

-- :LcvgcStatus
function M.status()
  M.send({ type = "status" }, function(response)
    if response.success then
      vim.notify("lcvgc: " .. (response.message or ""))
    end
  end)
end

-- コマンド登録
function M.register_commands()
  vim.api.nvim_create_user_command("LcvgcConnect", function() M.connect() end, {})
  vim.api.nvim_create_user_command("LcvgcDisconnect", function() M.disconnect() end, {})
  vim.api.nvim_create_user_command("LcvgcEval", function(opts) M.eval(opts) end, { range = true })
  vim.api.nvim_create_user_command("LcvgcLoad", function(opts) M.load(opts.args) end, { nargs = 1, complete = "file" })
  vim.api.nvim_create_user_command("LcvgcPlay", function(opts) M.play(opts.args) end, { nargs = 1 })
  vim.api.nvim_create_user_command("LcvgcStop", function(opts) M.stop(opts.args ~= "" and opts.args or nil) end, { nargs = "?" })
  vim.api.nvim_create_user_command("LcvgcStatus", function() M.status() end, {})
end

return M
