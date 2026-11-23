use br_runner_jobs_client::{NewMailAttachment, NewMailInput};
use uuid::Uuid;

#[test]
fn given_the_ids_and_the_texts_when_a_new_mail_input_is_made_then_the_recipients_and_attachments_are_empty_and_the_fields_can_be_set()
 {
    let from = Uuid::from_u128(2);
    let mut mail = NewMailInput::new(
        Uuid::from_u128(1),
        from,
        "Ann",
        Uuid::from_u128(3),
        "s",
        "b",
    );
    assert!(mail.to.is_empty() && mail.cc.is_empty() && mail.attachments.is_empty());
    mail.to.push(Uuid::from_u128(4));
    mail.attachments.push(NewMailAttachment {
        attachment_id: Uuid::from_u128(5),
        name: "f".to_owned(),
        content_type: "text/plain".to_owned(),
        content: "c".to_owned(),
    });
    assert_eq!(
        (
            mail.author.as_str(),
            mail.subject.as_str(),
            mail.body.as_str()
        ),
        ("Ann", "s", "b")
    );
    assert_eq!(mail.from_address, from);
    assert_eq!((mail.to.len(), mail.attachments.len()), (1, 1));
}
