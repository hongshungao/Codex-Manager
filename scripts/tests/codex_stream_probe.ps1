param(
    [string]$Base = "http://127.0.0.1:48760",
    [string]$ApiKey = "",
    [string]$Model = "gpt-5.3-codex",
    [string]$Prompt = "Reply exactly: PING",
    [int]$TimeoutSeconds = 90,
    [string]$OutDir = "",
    [string]$TraceLogPath = "$env:APPDATA\com.codexmanager.desktop\gateway-trace.log"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Require-Tool
{
    param([string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue))
    {
        throw "Missing required tool: $Name"
    }
}

function New-OutDir
{
    param([string]$InputOutDir)
    if ($InputOutDir -and $InputOutDir.Trim().Length -gt 0)
    {
        $dir = $InputOutDir
    } else
    {
        $stamp = Get-Date -Format "yyyyMMdd_HHmmss"
        $desktop = [Environment]::GetFolderPath("Desktop")
        $dir = Join-Path $desktop "codex_stream_probe_$stamp"
    }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    return $dir
}

function Build-ChatBodyJson
{
    param(
        [string]$BodyModel,
        [string]$BodyPrompt
    )
    $obj = @{
        model = $BodyModel
        stream = $true
        messages = @(
            @{
                role = "user"
                content = $BodyPrompt
            }
        )
    }
    return ($obj | ConvertTo-Json -Depth 20 -Compress)
}

function Build-ResponsesBodyJson
{
    param(
        [string]$BodyModel,
        [string]$BodyPrompt
    )
    $obj = @{
        model = $BodyModel
        stream = $true
        input = @(
            @{
                type = "message"
                role = "user"
                content = @(
                    @{
                        type = "input_text"
                        text = $BodyPrompt
                    }
                )
            }
        )
    }
    return ($obj | ConvertTo-Json -Depth 20 -Compress)
}

function Invoke-CurlStream
{
    param(
        [string]$Url,
        [string]$Key,
        [string]$BodyFile,
        [string]$HeaderFile,
        [string]$StreamFile,
        [int]$MaxSeconds
    )

    $args = @(
        "-sS", "-N",
        "-D", $HeaderFile,
        "-o", $StreamFile,
        "-X", "POST", $Url,
        "-H", "Authorization: Bearer $Key",
        "-H", "Content-Type: application/json",
        "--data-binary", "@$BodyFile",
        "--max-time", "$MaxSeconds"
    )

    & curl.exe @args
    if ($LASTEXITCODE -ne 0)
    {
        throw "curl failed for $Url with exit code $LASTEXITCODE"
    }
}

function Get-DataLinePayloads
{
    param([string]$Path)
    if (-not (Test-Path $Path))
    { return @() 
    }
    $lines = Get-Content -Path $Path -Encoding UTF8
    $payloads = @()
    foreach ($line in $lines)
    {
        if ($line -like "data:*")
        {
            $payloads += $line.Substring(5).Trim()
        }
    }
    return $payloads
}

function Read-ChunkText
{
    param($DeltaContent)
    if ($null -eq $DeltaContent)
    { return "" 
    }
    if ($DeltaContent -is [string])
    { return $DeltaContent 
    }
    if ($DeltaContent -is [System.Collections.IEnumerable])
    {
        $parts = @()
        foreach ($item in $DeltaContent)
        {
            if ($item -is [string])
            {
                $parts += $item
                continue
            }
            if ($item -and $item.PSObject.Properties.Match("text").Count -gt 0)
            {
                $parts += [string]$item.text
            }
        }
        return ($parts -join "")
    }
    return ""
}

function Has-Property
{
    param(
        $InputObject,
        [string]$PropertyName
    )
    if ($null -eq $InputObject)
    { return $false 
    }
    return ($InputObject.PSObject.Properties.Match($PropertyName).Count -gt 0)
}

function Analyze-ChatStream
{
    param([string]$Path)

    $payloads = Get-DataLinePayloads -Path $Path
    $doneSeen = $false
    $deltaChunks = New-Object System.Collections.Generic.List[string]
    $allRoleCount = 0
    $finishCount = 0
    $usageCount = 0
    $jsonCount = 0
    $badJson = 0
    $cumulativeLikeCount = 0

    $prev = ""
    foreach ($payload in $payloads)
    {
        if ($payload -eq "[DONE]")
        {
            $doneSeen = $true
            continue
        }
        $obj = $null
        try
        {
            $obj = $payload | ConvertFrom-Json -Depth 100
            $jsonCount++
        } catch
        {
            $badJson++
            continue
        }

        if ((Has-Property -InputObject $obj -PropertyName "usage") -and $null -ne $obj.usage)
        {
            $usageCount++
        }
        if ((Has-Property -InputObject $obj -PropertyName "choices") -and $obj.choices.Count -gt 0)
        {
            $choice0 = $obj.choices[0]
            if ((Has-Property -InputObject $choice0 -PropertyName "finish_reason") -and $null -ne $choice0.finish_reason)
            {
                $finishCount++
            }
            if ((Has-Property -InputObject $choice0 -PropertyName "delta") -and $null -ne $choice0.delta)
            {
                if (Has-Property -InputObject $choice0.delta -PropertyName "role")
                {
                    $allRoleCount++
                }
                $deltaContent = $null
                if (Has-Property -InputObject $choice0.delta -PropertyName "content")
                {
                    $deltaContent = $choice0.delta.content
                }
                $txt = Read-ChunkText -DeltaContent $deltaContent
                if ($txt.Length -gt 0)
                {
                    if ($prev.Length -ge 6 -and $txt.StartsWith($prev))
                    {
                        $cumulativeLikeCount++
                    }
                    $deltaChunks.Add($txt)
                    $prev = $txt
                }
            }
        }
    }

    $joined = [string]::Concat($deltaChunks)
    $distinctChunks = @($deltaChunks | Select-Object -Unique)
    $dupChunkCount = $deltaChunks.Count - $distinctChunks.Count

    $risk = $false
    if ($allRoleCount -gt 1)
    { $risk = $true 
    }
    if ($cumulativeLikeCount -gt 0)
    { $risk = $true 
    }
    if ($dupChunkCount -gt 0)
    { $risk = $true 
    }

    return [pscustomobject]@{
        done_seen = $doneSeen
        json_frames = $jsonCount
        bad_json_frames = $badJson
        role_frames = $allRoleCount
        finish_frames = $finishCount
        usage_frames = $usageCount
        delta_chunk_count = $deltaChunks.Count
        delta_chunk_distinct = $distinctChunks.Count
        repeated_delta_chunks = $dupChunkCount
        cumulative_like_chunks = $cumulativeLikeCount
        joined_delta_text_length = $joined.Length
        joined_delta_text_preview = if ($joined.Length -gt 300)
        { $joined.Substring(0, 300) 
        } else
        { $joined 
        }
        likely_duplicate_render_risk = $risk
    }
}

function Analyze-ResponsesStream
{
    param([string]$Path)
    $lines = @()
    if (Test-Path $Path)
    {
        $lines = Get-Content -Path $Path -Encoding UTF8
    }
    $eventLines = $lines | Where-Object { $_ -like "event:*" }
    $events = [System.Collections.Generic.Dictionary[string,int]]::new()
    foreach ($line in $eventLines)
    {
        $name = $line.Substring(6).Trim()
        if (-not $events.ContainsKey($name))
        {
            $events[$name] = 0
        }
        $events[$name]++
    }
    return [pscustomobject]@{
        event_line_count = $eventLines.Count
        event_counts = $events
    }
}

function Save-TraceTail
{
    param(
        [string]$Path,
        [string]$OutFile
    )
    if (-not (Test-Path $Path))
    {
        "trace log not found: $Path" | Set-Content -Path $OutFile -Encoding UTF8
        return
    }
    Get-Content -Path $Path -Tail 260 | Set-Content -Path $OutFile -Encoding UTF8
}

Require-Tool -Name "curl.exe"

if ([string]::IsNullOrWhiteSpace($ApiKey))
{
    $ApiKey = $env:CODEX_API_KEY
}
if ([string]::IsNullOrWhiteSpace($ApiKey))
{
    $ApiKey = $env:OPENAI_API_KEY
}
if ([string]::IsNullOrWhiteSpace($ApiKey))
{
    throw "ApiKey is empty. Pass -ApiKey or set CODEX_API_KEY/OPENAI_API_KEY."
}

$out = New-OutDir -InputOutDir $OutDir

$chatBodyPath = Join-Path $out "chat_body.json"
$respBodyPath = Join-Path $out "responses_body.json"
$chatHdrPath = Join-Path $out "chat_headers.txt"
$chatStreamPath = Join-Path $out "chat_stream.txt"
$respHdrPath = Join-Path $out "responses_headers.txt"
$respStreamPath = Join-Path $out "responses_stream.txt"
$traceOutPath = Join-Path $out "bridge_log.txt"
$summaryPath = Join-Path $out "summary.json"
$summaryTxtPath = Join-Path $out "summary.txt"

Build-ChatBodyJson -BodyModel $Model -BodyPrompt $Prompt | Set-Content -Path $chatBodyPath -Encoding UTF8
Build-ResponsesBodyJson -BodyModel $Model -BodyPrompt $Prompt | Set-Content -Path $respBodyPath -Encoding UTF8

Invoke-CurlStream -Url "$Base/v1/chat/completions" -Key $ApiKey -BodyFile $chatBodyPath -HeaderFile $chatHdrPath -StreamFile $chatStreamPath -MaxSeconds $TimeoutSeconds
Invoke-CurlStream -Url "$Base/v1/responses" -Key $ApiKey -BodyFile $respBodyPath -HeaderFile $respHdrPath -StreamFile $respStreamPath -MaxSeconds $TimeoutSeconds

Save-TraceTail -Path $TraceLogPath -OutFile $traceOutPath

$chat = Analyze-ChatStream -Path $chatStreamPath
$resp = Analyze-ResponsesStream -Path $respStreamPath

$summary = [pscustomobject]@{
    timestamp = (Get-Date).ToString("yyyy-MM-dd HH:mm:ss")
    base = $Base
    model = $Model
    prompt = $Prompt
    out_dir = $out
    chat = $chat
    responses = $resp
    files = [pscustomobject]@{
        chat_body = $chatBodyPath
        chat_headers = $chatHdrPath
        chat_stream = $chatStreamPath
        responses_body = $respBodyPath
        responses_headers = $respHdrPath
        responses_stream = $respStreamPath
        bridge_log = $traceOutPath
        summary_json = $summaryPath
        summary_txt = $summaryTxtPath
    }
}

$summary | ConvertTo-Json -Depth 100 | Set-Content -Path $summaryPath -Encoding UTF8

$text = @()
$text += "Base: $Base"
$text += "Model: $Model"
$text += "Prompt: $Prompt"
$text += "OutDir: $out"
$text += ""
$text += "Chat stream:"
$text += "  done_seen: $($chat.done_seen)"
$text += "  role_frames: $($chat.role_frames)"
$text += "  finish_frames: $($chat.finish_frames)"
$text += "  delta_chunk_count: $($chat.delta_chunk_count)"
$text += "  repeated_delta_chunks: $($chat.repeated_delta_chunks)"
$text += "  cumulative_like_chunks: $($chat.cumulative_like_chunks)"
$text += "  likely_duplicate_render_risk: $($chat.likely_duplicate_render_risk)"
$text += ""
$text += "Responses stream event line count: $($resp.event_line_count)"
$text += ""
$text += "Files:"
$text += "  chat_headers: $chatHdrPath"
$text += "  chat_stream: $chatStreamPath"
$text += "  responses_headers: $respHdrPath"
$text += "  responses_stream: $respStreamPath"
$text += "  bridge_log: $traceOutPath"
$text += "  summary_json: $summaryPath"
$text += "  summary_txt: $summaryTxtPath"

$text -join [Environment]::NewLine | Set-Content -Path $summaryTxtPath -Encoding UTF8

Write-Host "Done."
Write-Host "Output directory: $out"
Write-Host "Summary: $summaryTxtPath"
