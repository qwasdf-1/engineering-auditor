//! Kotlin, C#, C, and C++ capability boundaries for the catalog language
//! claim tests. Each negative is the nearest safe form of its positive, or
//! the adversarial form a rule must not report (`_exit` in a child, an
//! `ignored` catch, a dead initialization to a constant).

/// The capability files of `detector` for a Tier-2 `language`.
pub(super) fn tier2_capability_files(
    detector: &str,
    language: &str,
    positive: bool,
) -> Option<Vec<(String, String)>> {
    let source = match language {
        "kotlin" => kotlin_source(detector, positive)?,
        "csharp" => csharp_source(detector, positive)?,
        "c" => c_source(detector, positive, false)?,
        "cpp" => c_source(detector, positive, true)?,
        _ => return None,
    };
    let path = match language {
        "kotlin" => "src/main/kotlin/capability/Capability.kt",
        "csharp" => "src/Capability/Service.cs",
        "c" => "src/capability.c",
        _ => "src/capability.cpp",
    };
    Some(vec![(path.to_string(), source)])
}

fn pick<'a>(positive: bool, positive_body: &'a str, negative_body: &'a str) -> &'a str {
    if positive {
        positive_body
    } else {
        negative_body
    }
}

fn kotlin_source(detector: &str, positive: bool) -> Option<String> {
    if detector == "assert_in_production" {
        let visibility = if positive { "" } else { "private " };
        return Some(format!(
            "package capability\n\nclass Capability {{\n    {visibility}fun resize(size: Int) {{\n        assert(size > 0)\n    }}\n}}\n"
        ));
    }
    let (imports, positive_body, negative_body): (&str, &str, &str) = match detector {
        "bind_all_interfaces" => (
            "import java.net.InetSocketAddress",
            "val address = InetSocketAddress(\"0.0.0.0\", 8080)",
            "val address = InetSocketAddress(\"127.0.0.1\", 8080)",
        ),
        "console_output_in_production" => ("", "println(command)", "val text = command.trim()"),
        "empty_catch_block" => (
            "",
            "try { command.toInt() } catch (e: NumberFormatException) { }",
            "try { command.toInt() } catch (ignored: NumberFormatException) { }",
        ),
        "empty_conditional_branch" => (
            "",
            "if (command.isEmpty()) { } else { command.trim() }",
            "if (command.isEmpty()) { return }",
        ),
        "eval_in_production" => (
            "import javax.script.ScriptEngineManager",
            "ScriptEngineManager().getEngineByName(\"js\").eval(command)",
            "ScriptEngineManager().getEngineByName(\"js\").createBindings()",
        ),
        "exit_in_production" => (
            "import kotlin.system.exitProcess",
            "exitProcess(1)",
            "Runtime.getRuntime().gc()",
        ),
        "hardcoded_credential" => (
            "",
            "val password = \"hunter2-correct-horse\"",
            "val passwordEnv = \"SERVICE_PASSWORD\"",
        ),
        "hardcoded_tmp_directory" => (
            "",
            "val path = \"/tmp/capability.log\"",
            "val path = System.getProperty(\"java.io.tmpdir\")",
        ),
        "insecure_cryptography" => (
            "import java.security.MessageDigest",
            "MessageDigest.getInstance(\"MD5\")",
            "MessageDigest.getInstance(\"SHA-256\")",
        ),
        "random_for_security" => (
            "import java.security.SecureRandom\nimport java.util.Random",
            "val token = Random().nextInt()",
            "val token = SecureRandom().nextInt()",
        ),
        "request_no_cert_verify" => (
            "",
            "builder.hostnameVerifier { _, _ -> true }",
            "builder.hostnameVerifier { hostname, _ -> hostname == \"example.com\" }",
        ),
        "resource_not_closed" => (
            "import java.io.FileInputStream",
            "val input = FileInputStream(command)\n        input.read()",
            "val input = FileInputStream(command)\n        input.use { it.read() }",
        ),
        "shell_execution" => (
            "",
            "ProcessBuilder(\"sh\", \"-c\", command).start()",
            "ProcessBuilder(\"git\", \"status\", command).start()",
        ),
        "sql_injection" => (
            "",
            "statement.executeQuery(\"SELECT * FROM users WHERE name = '$command'\")",
            "statement.executeQuery(\"SELECT * FROM \" + TABLE_NAME)",
        ),
        "ssh_no_host_key_verification" => (
            "",
            "session.setConfig(\"StrictHostKeyChecking\", \"no\")",
            "session.setConfig(\"StrictHostKeyChecking\", \"yes\")",
        ),
        "ssl_with_bad_defaults" => (
            "import javax.net.ssl.SSLContext",
            "SSLContext.getInstance(\"TLSv1\")",
            "SSLContext.getInstance(\"TLSv1.3\")",
        ),
        "todo_unimplemented_in_production" => {
            ("", "TODO(\"parse $command\")", "error(\"unsupported $command\")")
        }
        "unsafe_deserialization" => (
            "import java.io.DataInputStream\nimport java.io.ObjectInputStream",
            "ObjectInputStream(stream).readObject()",
            "DataInputStream(stream).readInt()",
        ),
        "unused_assignment" => (
            "",
            "var count = command.length\n        count = 0\n        consume(count)",
            "var count = command.length\n        count += 1\n        consume(count)",
        ),
        "world_writable_file" => (
            "import java.io.File",
            "File(command).setWritable(true, false)",
            "File(command).setWritable(true, true)",
        ),
        "xml_xxe_vulnerable" => (
            "import javax.xml.parsers.DocumentBuilderFactory",
            "val factory = DocumentBuilderFactory.newInstance()\n        factory.newDocumentBuilder().parse(command)",
            "val factory = DocumentBuilderFactory.newInstance()\n        factory.setFeature(\"http://apache.org/xml/features/disallow-doctype-decl\", true)\n        factory.newDocumentBuilder().parse(command)",
        ),
        _ => return None,
    };
    Some(format!(
        "package capability\n\n{imports}\n\nclass Capability {{\n    fun run(command: String) {{\n        {}\n    }}\n}}\n",
        pick(positive, positive_body, negative_body)
    ))
}

fn csharp_source(detector: &str, positive: bool) -> Option<String> {
    if detector == "assert_in_production" {
        let visibility = if positive { "public" } else { "private" };
        return Some(format!(
            "using System.Diagnostics;\n\nnamespace Capability\n{{\n    public class Service\n    {{\n        {visibility} void Resize(int size)\n        {{\n            Debug.Assert(size > 0);\n        }}\n    }}\n}}\n"
        ));
    }
    let (usings, positive_body, negative_body): (&str, &str, &str) = match detector {
        "assignment_in_operand" => (
            "",
            "bool ready;\n            if (ready = Check(command)) { Go(); }",
            "string line;\n            while ((line = reader.ReadLine()) != null) { Go(); }",
        ),
        "bind_all_interfaces" => (
            "using System.Net;\nusing System.Net.Sockets;",
            "var listener = new TcpListener(IPAddress.Any, 8080);",
            "var listener = new TcpListener(IPAddress.Loopback, 8080);",
        ),
        "console_output_in_production" => (
            "using System.Diagnostics;",
            "Console.WriteLine(command);",
            "Trace.WriteLine(command);",
        ),
        "debugger_in_production" => (
            "using System.Diagnostics;",
            "Debugger.Break();",
            "Debugger.Log(0, \"capability\", command);",
        ),
        "empty_catch_block" => (
            "",
            "try { int.Parse(command); } catch (FormatException) { }",
            "try { int.Parse(command); } catch (FormatException ignored) { }",
        ),
        "empty_conditional_branch" => (
            "",
            "if (command.Length == 0) { } else { Go(); }",
            "if (command.Length == 0) { return; }",
        ),
        "eval_in_production" => (
            "using Microsoft.CodeAnalysis.CSharp.Scripting;",
            "CSharpScript.EvaluateAsync(command);",
            "CSharpScript.Create(command);",
        ),
        "exit_in_production" => (
            "",
            "Environment.Exit(1);",
            "Environment.GetEnvironmentVariable(command);",
        ),
        "hardcoded_credential" => (
            "",
            "string password = \"hunter2-correct-horse\";",
            "string passwordEnv = \"SERVICE_PASSWORD\";",
        ),
        "hardcoded_tmp_directory" => (
            "",
            "var path = \"/tmp/capability.log\";",
            "var path = Path.GetTempPath();",
        ),
        "insecure_cryptography" => (
            "using System.Security.Cryptography;",
            "var hash = MD5.Create();",
            "var hash = SHA256.Create();",
        ),
        "no_await_in_loop" => (
            "",
            "foreach (var item in items) { await SaveAsync(command); }",
            "foreach (var item in items) { await item.SaveAsync(); }",
        ),
        "random_for_security" => (
            "using System.Security.Cryptography;",
            "var random = new Random();",
            "var bytes = RandomNumberGenerator.GetBytes(16);",
        ),
        "request_no_cert_verify" => (
            "",
            "handler.ServerCertificateCustomValidationCallback = (message, cert, chain, errors) => true;",
            "handler.ServerCertificateCustomValidationCallback = (message, cert, chain, errors) => errors == SslPolicyErrors.None;",
        ),
        "resource_not_closed" => (
            "",
            "var stream = new FileStream(command, FileMode.Open);\n            stream.ReadByte();",
            "using var stream = new FileStream(command, FileMode.Open);\n            stream.ReadByte();",
        ),
        "shell_execution" => (
            "using System.Diagnostics;",
            "Process.Start(\"cmd.exe\", \"/c \" + command);",
            "Process.Start(\"git\", \"status \" + command);",
        ),
        "sql_injection" => (
            "using Microsoft.Data.SqlClient;",
            "var cmd = new SqlCommand(\"SELECT * FROM users WHERE name = '\" + command + \"'\", connection);",
            "var cmd = new SqlCommand(\"SELECT * FROM users WHERE name = @name\", connection);",
        ),
        "ssh_no_host_key_verification" => (
            "",
            "client.HostKeyReceived += (sender, e) => { e.CanTrust = true; };",
            "client.HostKeyReceived += (sender, e) => { e.CanTrust = e.FingerPrint.SequenceEqual(expected); };",
        ),
        "ssl_with_bad_defaults" => (
            "using System.Security.Authentication;",
            "var protocols = SslProtocols.Tls;",
            "var protocols = SslProtocols.Tls13;",
        ),
        "todo_unimplemented_in_production" => (
            "",
            "throw new NotImplementedException();",
            "throw new NotSupportedException();",
        ),
        "unsafe_deserialization" => (
            "using System.Runtime.Serialization.Formatters.Binary;",
            "var formatter = new BinaryFormatter();",
            "var options = new System.Text.Json.JsonSerializerOptions();",
        ),
        "unused_assignment" => (
            "",
            "int count = command.Length;\n            count = 0;\n            Go(count);",
            "int count = command.Length;\n            count += 1;\n            Go(count);",
        ),
        "world_writable_file" => (
            "",
            "File.SetUnixFileMode(command, UnixFileMode.UserRead | UnixFileMode.OtherWrite);",
            "File.SetUnixFileMode(command, UnixFileMode.UserRead | UnixFileMode.UserWrite);",
        ),
        "xml_xxe_vulnerable" => (
            "using System.Xml;",
            "var settings = new XmlReaderSettings { DtdProcessing = DtdProcessing.Parse };",
            "var settings = new XmlReaderSettings { DtdProcessing = DtdProcessing.Prohibit };",
        ),
        _ => return None,
    };
    Some(format!(
        "{usings}\n\nnamespace Capability\n{{\n    public class Service\n    {{\n        public async Task Run(string command)\n        {{\n            {}\n        }}\n    }}\n}}\n",
        pick(positive, positive_body, negative_body)
    ))
}

fn c_source(detector: &str, positive: bool, cpp: bool) -> Option<String> {
    let (positive_body, negative_body): (&str, &str) = match detector {
        "assignment_in_operand" => (
            "int n;\n    if (n = atoi(command)) { run_next(n); }",
            "int n;\n    if ((n = atoi(command))) { run_next(n); }",
        ),
        "bind_all_interfaces" => (
            "struct sockaddr_in addr;\n    addr.sin_addr.s_addr = htonl(INADDR_ANY);",
            "struct sockaddr_in addr;\n    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);",
        ),
        "console_output_in_production" => (
            "printf(\"%s\\n\", command);",
            "snprintf(buffer, sizeof buffer, \"%s\", command);",
        ),
        "debugger_in_production" => (
            "__builtin_debugtrap();",
            "__builtin_expect(command != 0, 1);\n    __builtin_debugtrap();\n    abort();",
        ),
        "empty_catch_block" if cpp => (
            "try { run_next(1); } catch (...) { }",
            "try { run_next(1); } catch (...) { /* best effort */ }",
        ),
        "empty_conditional_branch" => (
            "if (command == 0) { } else { run_next(1); }",
            "if (command == 0) { return 1; }",
        ),
        "exit_in_production" => ("exit(1);", "_exit(127);"),
        "hardcoded_credential" => (
            "const char *password = \"hunter2-correct-horse\";",
            "const char *password_env = \"SERVICE_PASSWORD\";",
        ),
        "hardcoded_tmp_directory" => (
            "const char *path = \"/tmp/capability.log\";",
            "const char *path = getenv(\"TMPDIR\");",
        ),
        "insecure_cryptography" => (
            "MD5((const unsigned char *)command, 4, digest);",
            "SHA256((const unsigned char *)command, 4, digest);",
        ),
        "random_for_security" => ("int token = rand();", "int token = getpid();"),
        "request_no_cert_verify" => (
            "curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 0L);",
            "curl_easy_setopt(curl, CURLOPT_SSL_VERIFYPEER, 1L);",
        ),
        "resource_not_closed" if !cpp => (
            "FILE *file = fopen(command, \"r\");\n    fgetc(file);",
            "FILE *file = fopen(command, \"r\");\n    fgetc(file);\n    fclose(file);",
        ),
        "shell_execution" => ("system(command);", "system(NULL);"),
        "ssh_no_host_key_verification" => (
            "int strict = 0;\n    ssh_options_set(session, SSH_OPTIONS_STRICTHOSTKEYCHECK, &strict);",
            "int strict = 1;\n    ssh_options_set(session, SSH_OPTIONS_STRICTHOSTKEYCHECK, &strict);",
        ),
        "ssl_with_bad_defaults" => (
            "SSL_CTX *ctx = SSL_CTX_new(TLSv1_method());",
            "SSL_CTX *ctx = SSL_CTX_new(TLS_method());",
        ),
        "todo_unimplemented_in_production" => (
            "/* TODO: validate the command */",
            "/* Validates the command. */",
        ),
        "unused_assignment" => (
            "int count = atoi(command);\n    count = 2;\n    run_next(count);",
            "int count = 0;\n    count = atoi(command);\n    run_next(count);",
        ),
        "world_writable_file" => ("chmod(command, 0777);", "chmod(command, 0755);"),
        "xml_xxe_vulnerable" => (
            "xmlDocPtr doc = xmlReadFile(command, NULL, XML_PARSE_NOENT);",
            "xmlDocPtr doc = xmlReadFile(command, NULL, XML_PARSE_NONET);",
        ),
        _ => return None,
    };
    let body = pick(positive, positive_body, negative_body);
    Some(if cpp {
        format!("#include <cstdlib>\n\nnamespace capability {{\nint run(const char *command) {{\n    {body}\n    return 0;\n}}\n}}\n")
    } else {
        format!("#include <stdlib.h>\n\nint run(const char *command) {{\n    {body}\n    return 0;\n}}\n")
    })
}
