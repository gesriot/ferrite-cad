// SPDX-License-Identifier: MIT
pub fn viewed(ticket: ferritecad_jobs::StepTicket) -> ferritecad_jobs::StepTicket {
    if ticket.source().rebuild_facts().is_none() {
        ticket
            .source()
            .load_scene(
                &mut ferritecad_kernel::mock::MockKernel::new(),
                |_, _| {
                    Err(ferritecad_types::CadError::unsupported(
                        "no imports in this test",
                    ))
                },
                &ferritecad_kernel::TessellationParams::default(),
                &ferritecad_kernel::OperationContext::default(),
            )
            .expect("opened session picture");
    }
    ticket
}
