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

-- マイク入力ジョブID
local mic_job_id = nil

-- :LcvgcMicStart - マイク入力開始、カーソル位置にDSLテキストを挿入
function M.mic_start(opts)
  if mic_job_id then
    vim.notify("lcvgc-mic: already running (job " .. mic_job_id .. ")", vim.log.levels.WARN)
    return
  end

  local cmd = { "lcvgc-mic" }
  local args = opts and opts.args or ""
  if args ~= "" then
    for arg in args:gmatch("%S+") do
      table.insert(cmd, arg)
    end
  end

  mic_job_id = vim.fn.jobstart(cmd, {
    stdout_buffered = false,
    on_stdout = function(_job_id, data, _event)
      vim.schedule(function()
        if not data then return end
        for _, line in ipairs(data) do
          if line ~= "" then
            -- カーソル位置にテキストを挿入
            local row, col = unpack(vim.api.nvim_win_get_cursor(0))
            local current_line = vim.api.nvim_buf_get_lines(0, row - 1, row, false)[1] or ""
            local before = current_line:sub(1, col)
            local after = current_line:sub(col + 1)
            vim.api.nvim_buf_set_lines(0, row - 1, row, false, { before .. line .. after })
            vim.api.nvim_win_set_cursor(0, { row, col + #line })
          end
        end
      end)
    end,
    on_stderr = function(_job_id, data, _event)
      vim.schedule(function()
        if data then
          for _, line in ipairs(data) do
            if line ~= "" then
              vim.notify("lcvgc-mic: " .. line, vim.log.levels.INFO)
            end
          end
        end
      end)
    end,
    on_exit = function(_job_id, exit_code, _event)
      vim.schedule(function()
        mic_job_id = nil
        if exit_code ~= 0 then
          vim.notify("lcvgc-mic: exited with code " .. exit_code, vim.log.levels.WARN)
        else
          vim.notify("lcvgc-mic: stopped")
        end
      end)
    end,
  })

  if mic_job_id <= 0 then
    vim.notify("lcvgc-mic: failed to start", vim.log.levels.ERROR)
    mic_job_id = nil
  else
    vim.notify("lcvgc-mic: started (job " .. mic_job_id .. ")")
  end
end

-- :LcvgcMicStop - マイク入力停止
function M.mic_stop()
  if not mic_job_id then
    vim.notify("lcvgc-mic: not running", vim.log.levels.WARN)
    return
  end

  vim.fn.jobstop(mic_job_id)
  mic_job_id = nil
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
  vim.api.nvim_create_user_command("LcvgcMicStart", function(opts) M.mic_start(opts) end, { nargs = "*" })
  vim.api.nvim_create_user_command("LcvgcMicStop", function() M.mic_stop() end, {})

  -- キーバインド: マイク入力
  vim.keymap.set("n", "<leader>ms", ":LcvgcMicStart<CR>", { silent = true, desc = "lcvgc-mic start" })
  vim.keymap.set("n", "<leader>mx", ":LcvgcMicStop<CR>", { silent = true, desc = "lcvgc-mic stop" })
end

return M
