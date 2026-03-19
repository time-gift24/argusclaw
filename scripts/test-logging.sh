#!/bin/bash

# 测试日志初始化和输出

echo "🔍 Testing ArgusWing logging"
echo "---"

# 清理旧日志
rm -f ./tmp/arguswing.log

# 设置日志级别为 trace
export RUST_LOG=arguswing=trace,argus=trace,argus_llm=trace

echo "🚀 Starting Tauri app with trace logging..."
echo "📝 Log file: ./tmp/arguswing.log"
echo ""
echo "💡 To test provider connection:"
echo "   1. Open the Tauri app"
echo "   2. Go to Settings > Providers"
echo "   3. Click '测试连接' on any provider"
echo "   4. Check the log file for detailed output"
echo ""
echo "📋 Monitor logs in real-time:"
echo "   tail -f ./tmp/arguswing.log"
echo ""
echo "🔑 Look for these log entries:"
echo "   - full openai-compatible response payload"
echo "   - provider test received response"
echo "   - content_preview"
echo ""

# 启动 Tauri 应用（如果需要）
# pnpm tauri dev
