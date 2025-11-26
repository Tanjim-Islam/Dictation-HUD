#!/usr/bin/env node

/**
 * Standalone API test script
 * Tests Deepgram and OpenRouter independently
 */

import fs from 'fs';
import https from 'https';
import http from 'http';
import { URL } from 'url';

const AUDIO_FILE = './Recording.mp3';
const OUTPUT_FILE = './test-results.txt';

// Get keys from command line or environment
const DEEPGRAM_KEY = process.env.DEEPGRAM_KEY || process.argv[2];
const OPENROUTER_KEY = process.env.OPENROUTER_KEY || process.argv[3];

if (!DEEPGRAM_KEY || !OPENROUTER_KEY) {
  console.error('❌ Usage: node test-apis.mjs <DEEPGRAM_KEY> <OPENROUTER_KEY>');
  console.error('   OR: DEEPGRAM_KEY=xxx OPENROUTER_KEY=yyy node test-apis.mjs');
  process.exit(1);
}

console.log('🔑 Deepgram key:', DEEPGRAM_KEY.substring(0, 10) + '...');
console.log('🔑 OpenRouter key:', OPENROUTER_KEY.substring(0, 15) + '...');

// Helper to make HTTPS requests
function httpsRequest(url, options, postData = null) {
  return new Promise((resolve, reject) => {
    const urlObj = new URL(url);
    const opts = {
      hostname: urlObj.hostname,
      path: urlObj.pathname + urlObj.search,
      method: options.method || 'GET',
      headers: options.headers || {},
      ...options
    };

    const req = https.request(opts, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        if (res.statusCode >= 200 && res.statusCode < 300) {
          try {
            resolve(JSON.parse(data));
          } catch (e) {
            resolve(data);
          }
        } else {
          reject(new Error(`HTTP ${res.statusCode}: ${data}`));
        }
      });
    });

    req.on('error', reject);

    if (postData) {
      if (Buffer.isBuffer(postData)) {
        req.write(postData);
      } else {
        req.write(JSON.stringify(postData));
      }
    }

    req.end();
  });
}

// Test 1: Deepgram transcription
async function testDeepgram() {
  console.log('\n📡 TEST 1: Deepgram Speech-to-Text');
  console.log('=' .repeat(60));

  try {
    // Check if audio file exists
    if (!fs.existsSync(AUDIO_FILE)) {
      throw new Error(`Audio file not found: ${AUDIO_FILE}`);
    }

    const audioBuffer = fs.readFileSync(AUDIO_FILE);
    console.log(`📁 Audio file size: ${audioBuffer.length} bytes`);

    // Send to Deepgram REST API
    console.log('🌐 Sending to Deepgram API...');

    const url = 'https://api.deepgram.com/v1/listen?model=nova-2&language=en&smart_format=true&punctuate=true';

    const result = await httpsRequest(url, {
      method: 'POST',
      headers: {
        'Authorization': `Token ${DEEPGRAM_KEY}`,
        'Content-Type': 'audio/mpeg'
      }
    }, audioBuffer);

    console.log('✅ Deepgram response received');
    console.log('📝 Full response:', JSON.stringify(result, null, 2));

    // Extract transcript
    const transcript = result?.results?.channels?.[0]?.alternatives?.[0]?.transcript;

    if (!transcript) {
      throw new Error('No transcript in response');
    }

    console.log('\n✅ DEEPGRAM SUCCESS');
    console.log('📝 Transcript:', transcript);

    return transcript;

  } catch (error) {
    console.log('\n❌ DEEPGRAM FAILED');
    console.log('Error:', error.message);
    if (error.message.includes('401')) {
      console.log('💡 This likely means your Deepgram API key is invalid or expired');
    }
    throw error;
  }
}

// Test 2: OpenRouter refinement
async function testOpenRouter(rawText) {
  console.log('\n🤖 TEST 2: OpenRouter Text Refinement');
  console.log('=' .repeat(60));

  try {
    console.log('📝 Input text:', rawText);
    console.log('🌐 Sending to OpenRouter API...');

    const body = {
      model: 'openai/gpt-oss-20b:free',
      messages: [
        {
          role: 'system',
          content:
            'You are a dictation refiner. Keep meaning. Add punctuation. Fix capitalization. Fix obvious homophones only if confident. Do not add or remove names, numbers, URLs, or commands. Return plain text only.',
        },
        {
          role: 'user',
          content: rawText,
        },
      ],
    };

    const result = await httpsRequest('https://openrouter.ai/api/v1/chat/completions', {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${OPENROUTER_KEY}`,
        'Content-Type': 'application/json'
      }
    }, body);

    console.log('✅ OpenRouter response received');

    const refined = result?.choices?.[0]?.message?.content;

    if (!refined) {
      throw new Error('No content in response');
    }

    console.log('\n✅ OPENROUTER SUCCESS');
    console.log('📝 Refined text:', refined);

    return refined;

  } catch (error) {
    console.log('\n❌ OPENROUTER FAILED');
    console.log('Error:', error.message);
    if (error.message.includes('401')) {
      console.log('💡 This likely means your OpenRouter API key is invalid');
    }
    throw error;
  }
}

// Main test flow
async function main() {
  const results = [];

  try {
    // Test 1: Deepgram
    const transcript = await testDeepgram();
    results.push(`DEEPGRAM TEST: ✅ PASS`);
    results.push(`Transcript: ${transcript}`);

    // Test 2: OpenRouter
    const refined = await testOpenRouter(transcript);
    results.push(`\nOPENROUTER TEST: ✅ PASS`);
    results.push(`Refined: ${refined}`);

    console.log('\n' + '='.repeat(60));
    console.log('✅ ALL TESTS PASSED');
    console.log('='.repeat(60));

    // Write results to file
    fs.writeFileSync(OUTPUT_FILE, results.join('\n'));
    console.log(`\n📄 Results saved to: ${OUTPUT_FILE}`);

    return true;

  } catch (error) {
    results.push(`\n❌ TESTS FAILED: ${error.message}`);
    fs.writeFileSync(OUTPUT_FILE, results.join('\n'));
    console.log(`\n📄 Error log saved to: ${OUTPUT_FILE}`);
    return false;
  }
}

main().then(success => process.exit(success ? 0 : 1));
