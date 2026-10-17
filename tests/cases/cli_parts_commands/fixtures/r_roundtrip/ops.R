counter <- 0

transform <- function(input) {
  counter <<- counter + 1
  list(count = counter, nested = input$nested, vector = input$vector, scalar = input$scalar, nothing = NULL)
}

double_values <- function(input) input * 2

scale_table <- function(input) {
  if (!is.data.frame(input)) stop("typed table did not arrive as an R data.frame")
  input$score <- input$score * 2
  input
}

plot_scores <- function(input) {
  values <- unlist(input$values, use.names = FALSE)
  plot(values, type = "b", main = "Jet and R", xlab = "sample", ylab = "score")
}

fail_call <- function(input) stop("raw secret failure detail")
sleep_call <- function(input) { Sys.sleep(30); input }

replace_plot <- function(value) {
  device <- dev.cur()
  dev.off(device)
  writeChar(value, file.path(Sys.getenv("JET_BIND_TEMP"), "plot.svg"), eos = NULL, useBytes = TRUE)
}
hostile_plot <- function(input) {
  kind <- input$kind
  value <- switch(kind,
    script = '<svg xmlns="http://www.w3.org/2000/svg"><script>raw secret script</script></svg>',
    event = '<svg xmlns="http://www.w3.org/2000/svg" onload="raw secret event"><path d="M0 0"/></svg>',
    foreign = '<svg xmlns="http://www.w3.org/2000/svg"><foreignObject>raw secret foreign</foreignObject></svg>',
    external = '<svg xmlns="http://www.w3.org/2000/svg"><use href="https://evil.invalid/raw-secret"/></svg>',
    css = '<svg xmlns="http://www.w3.org/2000/svg"><path style="fill:url(https://evil.invalid/raw-secret)"/></svg>',
    doctype = '<!DOCTYPE svg [<!ENTITY xxe SYSTEM "file:///raw-secret">]><svg xmlns="http://www.w3.org/2000/svg">&xxe;</svg>',
    malformed = '<svg xmlns="http://www.w3.org/2000/svg"><path></svg>',
    oversize = paste0('<svg xmlns="http://www.w3.org/2000/svg"><desc>', strrep('x', 524288), '</desc></svg>'),
    stop('unknown hostile plot'))
  replace_plot(value)
}
